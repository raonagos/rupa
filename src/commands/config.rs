use std::path::Path;

use anyhow::{bail, Result};

use crate::cli::ConfigCmd;
use crate::db;
use crate::ip;

pub fn run(db_path: &Path, command: ConfigCmd) -> Result<()> {
    match command {
        ConfigCmd::Set {
            v6_subnet,
            no_ipv6,
            full_tunnel,
        } => set(db_path, v6_subnet, no_ipv6, full_tunnel),
    }
}

fn set(
    db_path: &Path,
    v6_subnet: Option<String>,
    no_ipv6: bool,
    full_tunnel: Option<String>,
) -> Result<()> {
    let full_tunnel = match full_tunnel.as_deref() {
        None => None,
        Some("on") | Some("true") | Some("yes") => Some(true),
        Some("off") | Some("false") | Some("no") => Some(false),
        Some(other) => bail!("--full-tunnel expects on or off, got '{other}'"),
    };

    let v6 = if no_ipv6 {
        Some(None)
    } else {
        v6_subnet
            .as_deref()
            .map(|value| ip::parse_v6_subnet(value).map(Some))
            .transpose()?
    };

    if v6.is_none() && full_tunnel.is_none() {
        bail!("nothing to change: pass --v6-subnet, --no-ipv6 or --full-tunnel on|off");
    }

    let mut conn = db::open(db_path)?;
    let server = db::load_server(&conn)?;
    db::update_network(&mut conn, server.id, v6, full_tunnel)?;
    let updated = db::load_server(&conn)?;

    println!("Server {} ({})", updated.name, updated.interface);
    match (updated.v6_address, updated.v6_subnet) {
        (Some(address), Some(subnet)) => {
            println!("  ipv6:       {address}/{} ({subnet})", subnet.prefix_len())
        }
        _ => println!("  ipv6:       disabled"),
    }
    println!(
        "  tunnel:     {}",
        if updated.full_tunnel {
            "full (NAT)"
        } else {
            "split"
        }
    );
    println!();
    println!("Restart required: sudo systemctl restart rupa");
    Ok(())
}
