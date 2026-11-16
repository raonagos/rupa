use std::net::Ipv4Addr;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::json;

use crate::cli::DnsCmd;
use crate::control;
use crate::db;
use crate::model::DnsSource;
use crate::util;

pub fn run(db_path: &Path, command: DnsCmd) -> Result<()> {
    match command {
        DnsCmd::Add { name, address } => add(db_path, &name, &address),
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
