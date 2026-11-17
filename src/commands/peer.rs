use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::cli::{PeerAddArgs, PeerCmd};
use crate::control;
use crate::db;
use crate::ip;
use crate::keys;
use crate::model::{DnsSource, Server};
use crate::peer_config;
use crate::util;

pub fn run(db_path: &Path, command: PeerCmd) -> Result<()> {
    match command {
        PeerCmd::Add(args) => add(db_path, args),
        PeerCmd::List { live, json } => list(db_path, live, json),
        PeerCmd::Show { name, qr } => show(db_path, &name, qr),
        PeerCmd::Remove { name, yes } => remove(db_path, &name, yes),
    }
}

fn add(db_path: &Path, args: PeerAddArgs) -> Result<()> {
    let mut conn = db::open(db_path)?;
    let server = db::load_server(&conn)?;
    let name = util::validate_peer_name(&args.name)?;
    if db::peer_exists(&conn, server.id, &name)? {
        bail!("peer '{name}' already exists");
    }

    let used = db::used_addresses(&conn, server.id)?;
    let address = ip::allocate(&server.subnet, server.address, &used)?;
    let pair = keys::generate()?;
    let dns_name = match args.dns.as_deref() {
        Some(value) => Some(resolve_dns_name(&server, value)?),
        None => None,
    };
    let created_at = util::now_rfc3339();

    let tx = conn.transaction()?;
    if let Some(dns) = &dns_name {
        if let Some(existing) = db::find_dns_record(&tx, server.id, dns)? {
            bail!(
                "DNS name '{dns}' is already used by a {} record",
                existing.source.as_str()
            );
        }
    }
    let peer_id = db::insert_peer(
        &tx,
        server.id,
        &name,
        &pair.private,
        &pair.public,
        address,
        &created_at,
    )?;
    if let Some(dns) = &dns_name {
        db::upsert_dns_record(
            &tx,
            server.id,
            dns,
            address,
            DnsSource::Peer,
            Some(peer_id),
            &created_at,
        )?;
    }
    tx.commit()?;

    let peer = db::find_peer(&conn, server.id, &name)?
        .context("peer disappeared right after insertion")?;
    let config = peer_config::build(&server, &peer)?;

    if let Some(path) = args.output.as_deref() {
        write_private(path, &config)?;
        println!("Wrote {} (0600)", path.display());
        if args.qr {
            print_qr(&config)?;
        }
    } else if args.qr {
        print_qr(&config)?;
    } else {
        print!("{config}");
    }

    control::notify_reload();
    Ok(())
}

fn list(db_path: &Path, live: bool, json_output: bool) -> Result<()> {
    let conn = db::open(db_path)?;
    let server = db::load_server(&conn)?;
    let peers = db::list_peers(&conn, server.id)?;
    let stats = if live {
        control::request("stats").ok()
    } else {
        None
    };

    if live && stats.is_none() {
        eprintln!(
            "note: --live requested but the server is not running; showing the database state"
        );
    }

    if json_output {
        let values: Vec<_> = peers
            .iter()
            .map(|entry| {
                let public_key = entry.peer.public_key_b64().ok();
                let live_value = stats.as_ref().and_then(|stats| {
                    let key = public_key.as_deref()?;
                    stats["peers"]
                        .as_array()?
                        .iter()
                        .find(|peer| peer["public_key"].as_str() == Some(key))
                });
                json!({
                    "name": &entry.peer.name,
                    "address": entry.peer.address.to_string(),
                    "dns": entry.dns_name.as_deref(),
                    "public_key": public_key,
                    "created_at": &entry.peer.created_at,
                    "enabled": entry.peer.enabled,
                    "live": live_value,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&values)?);
        return Ok(());
    }

    if peers.is_empty() {
        println!("No peers.");
        return Ok(());
    }

    if let Some(stats) = &stats {
        let by_key: HashMap<&str, &Value> = stats["peers"]
            .as_array()
            .map(|peers| {
                peers
                    .iter()
                    .filter_map(|peer| peer["public_key"].as_str().map(|key| (key, peer)))
                    .collect()
            })
            .unwrap_or_default();

        println!(
            "{:<18} {:<14} {:<30} {:<10} {:<10} {:<10} {:<10}",
            "NAME", "ADDRESS", "DNS", "CREATED", "HANDSHAKE", "RX", "TX"
        );
        for entry in &peers {
            let key = entry.peer.public_key_b64()?;
            let live_peer = by_key.get(key.as_str());
            let handshake = live_peer
                .and_then(|peer| peer["last_handshake"].as_u64())
                .map(util::format_age_epoch)
                .unwrap_or_else(|| "never".into());
            let rx = util::format_bytes(
                live_peer
                    .and_then(|peer| peer["rx_bytes"].as_u64())
                    .unwrap_or(0),
            );
            let tx = util::format_bytes(
                live_peer
                    .and_then(|peer| peer["tx_bytes"].as_u64())
                    .unwrap_or(0),
            );
            println!(
                "{:<18} {:<14} {:<30} {:<10} {:<10} {:<10} {:<10}",
                entry.peer.name,
                entry.peer.address,
                entry.dns_name.as_deref().unwrap_or("-"),
                util::short_date(&entry.peer.created_at),
                handshake,
                rx,
                tx,
            );
        }
        return Ok(());
    }

    println!(
        "{:<20} {:<15} {:<34} {:<10}",
        "NAME", "ADDRESS", "DNS", "CREATED"
    );
    for entry in &peers {
        println!(
            "{:<20} {:<15} {:<34} {:<10}",
            entry.peer.name,
            entry.peer.address,
            entry.dns_name.as_deref().unwrap_or("-"),
            util::short_date(&entry.peer.created_at),
        );
    }
    Ok(())
}

fn show(db_path: &Path, name: &str, qr: bool) -> Result<()> {
    let conn = db::open(db_path)?;
    let server = db::load_server(&conn)?;
    let name = util::validate_peer_name(name)?;
    let peer = db::find_peer(&conn, server.id, &name)?
        .with_context(|| format!("peer '{name}' not found"))?;
    let config = peer_config::build(&server, &peer)?;
    if qr {
        print_qr(&config)?;
    } else {
        print!("{config}");
    }
    Ok(())
}

fn remove(db_path: &Path, name: &str, yes: bool) -> Result<()> {
    let conn = db::open(db_path)?;
    let server = db::load_server(&conn)?;
    let name = util::validate_peer_name(name)?;
    let peer = db::find_peer(&conn, server.id, &name)?
        .with_context(|| format!("peer '{name}' not found"))?;

    if !yes
        && !util::confirm(&format!(
            "Remove peer '{}' ({}) and its DNS record?",
            peer.name, peer.address
        ))?
    {
        println!("Aborted.");
        return Ok(());
    }

    if !db::delete_peer(&conn, server.id, &name)? {
        bail!("peer '{name}' was not deleted");
    }
    println!("Removed peer {} ({})", peer.name, peer.address);
    control::notify_reload();
    Ok(())
}

fn resolve_dns_name(server: &Server, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("--dns must not be empty");
    }
    let fqdn = if value.contains('.') {
        value.to_string()
    } else {
        let zone = server.dns_zone.as_deref().context(
            "--dns with a label requires --dns-zone at init; pass a fully qualified name instead",
        )?;
        format!("{value}.{zone}")
    };
    util::validate_dns_name(&fqdn)
}

fn write_private(path: &Path, contents: &str) -> Result<()> {
    fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))?;
    let mut perms = fs::metadata(path)
        .with_context(|| format!("failed to stat {}", path.display()))?
        .permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)
        .with_context(|| format!("failed to chmod {}", path.display()))?;
    Ok(())
}

fn print_qr(text: &str) -> Result<()> {
    use qrcode::render::unicode::Dense1x2;

    let code = qrcode::QrCode::new(text.as_bytes()).context("failed to build the QR code")?;
    let mut renderer = code.render::<Dense1x2>();
    renderer.quiet_zone(true);
    let image = renderer.build();
    println!("{image}");
    Ok(())
}
