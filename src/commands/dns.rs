use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::net::Ipv4Addr;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::json;

use crate::cli::DnsCmd;
use crate::control;
use crate::db;
use crate::dns::import::parse_hosts;
use crate::model::DnsSource;
use crate::util;

pub fn run(db_path: &Path, command: DnsCmd) -> Result<()> {
    match command {
        DnsCmd::Add { name, address } => add(db_path, &name, &address),
        DnsCmd::Import {
            path,
            source,
            prune,
        } => import(db_path, &path, &source, prune),
        DnsCmd::List { json } => list(db_path, json),
        DnsCmd::Remove { name, yes } => remove(db_path, &name, yes),
    }
}

fn add(db_path: &Path, name: &str, address: &str) -> Result<()> {
    let conn = db::open(db_path)?;
    let server = db::load_server(&conn)?;
    let name = util::validate_dns_name(name)?;
    let address: Ipv4Addr = address
        .trim()
        .parse()
        .with_context(|| format!("invalid IPv4 address '{address}'"))?;

    let existing = db::find_dns_record(&conn, server.id, &name)?;
    if let Some(record) = &existing {
        if record.source == DnsSource::Peer {
            bail!("DNS name '{name}' is managed by a peer; remove the peer instead");
        }
    }

    db::upsert_dns_record(
        &conn,
        server.id,
        &name,
        address,
        DnsSource::Manual,
        None,
        &util::now_rfc3339(),
    )?;
    println!(
        "{} DNS record {name} -> {address}",
        if existing.is_some() {
            "Updated"
        } else {
            "Added"
        }
    );
    control::notify_reload();
    Ok(())
}

fn list(db_path: &Path, json_output: bool) -> Result<()> {
    let conn = db::open(db_path)?;
    let server = db::load_server(&conn)?;
    let records = db::list_dns_records(&conn, server.id)?;

    if json_output {
        let values: Vec<_> = records
            .iter()
            .map(|record| {
                json!({
                    "name": &record.name,
                    "address": record.address.to_string(),
                    "source": record.source.as_str(),
                    "updated_at": &record.updated_at,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&values)?);
        return Ok(());
    }

    if records.is_empty() {
        println!("No DNS records.");
        return Ok(());
    }

    println!("{:<40} {:<15} {:<8}", "NAME", "ADDRESS", "SOURCE");
    for record in &records {
        println!(
            "{:<40} {:<15} {:<8}",
            record.name,
            record.address,
            record.source.as_str()
        );
    }
    Ok(())
}

fn remove(db_path: &Path, name: &str, yes: bool) -> Result<()> {
    let conn = db::open(db_path)?;
    let server = db::load_server(&conn)?;
    let name = util::validate_dns_name(name)?;
    let record = db::find_dns_record(&conn, server.id, &name)?.context("DNS record not found")?;

    if record.source == DnsSource::Peer {
        bail!("DNS name '{name}' is managed by a peer; remove the peer instead");
    }
    if !yes
        && !util::confirm(&format!(
            "Remove DNS record '{}' -> {}?",
            record.name, record.address
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    if !db::delete_dns_record(&conn, server.id, &name)? {
        bail!("DNS record '{name}' was not deleted");
    }
    println!("Removed DNS record {}", record.name);
    control::notify_reload();
    Ok(())
}

fn import(db_path: &Path, path: &str, source: &str, prune: bool) -> Result<()> {
    let source = match source {
        "uping" => DnsSource::Uping,
        "manual" => DnsSource::Manual,
        other => bail!("unsupported import source '{other}' (use uping or manual)"),
    };

    let input = if path == "-" {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .context("failed to read stdin")?;
        buffer
    } else {
        fs::read_to_string(path).with_context(|| format!("failed to read {path}"))?
    };

    let parsed = parse_hosts(&input);
    for warning in &parsed.warnings {
        eprintln!("warning: {warning}");
    }
    if parsed.entries.is_empty() {
        bail!("no valid DNS record found in {path}");
    }

    let mut entries: BTreeMap<String, Ipv4Addr> = BTreeMap::new();
    for entry in parsed.entries {
        entries.insert(entry.name, entry.address);
    }

    let mut conn = db::open(db_path)?;
    let server = db::load_server(&conn)?;
    let tx = conn.transaction()?;
    let mut added = 0usize;
    let mut updated = 0usize;

    for (name, address) in &entries {
        if let Some(existing) = db::find_dns_record(&tx, server.id, name)? {
            if existing.source == DnsSource::Peer {
                bail!("DNS name '{name}' is managed by a peer; remove the peer instead");
            }
            updated += 1;
        } else {
            added += 1;
        }
        db::upsert_dns_record(
            &tx,
            server.id,
            name,
            *address,
            source,
            None,
            &util::now_rfc3339(),
        )?;
    }

    let mut pruned = 0usize;
    if prune {
        let existing = db::list_dns_records(&tx, server.id)?;
        for record in existing {
            if record.source == source
                && !entries.contains_key(&record.name)
                && db::delete_dns_record(&tx, server.id, &record.name)?
            {
                pruned += 1;
            }
        }
    }
    tx.commit()?;

    let prune_note = if prune {
        format!(", {pruned} pruned")
    } else {
        String::new()
    };
    println!(
        "Imported {} records ({added} new, {updated} overwritten{prune_note})",
        entries.len()
    );
    control::notify_reload();
    Ok(())
}
