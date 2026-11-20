use std::path::Path;

use anyhow::Result;
use serde_json::json;

use crate::{control, db, util};

pub fn run(db_path: &Path, json_output: bool) -> Result<()> {
    let conn = db::open(db_path)?;
    let server = db::load_server(&conn)?;
    let peers = db::list_peers(&conn, server.id)?;
    let records = db::list_dns_records(&conn, server.id)?;
    let runtime_stats = if control::is_running() {
        control::request("stats").ok()
    } else {
        None
    };
    let running = runtime_stats.is_some();

    if json_output {
        let value = json!({
            "db": db_path.display().to_string(),
            "runtime": running,
            "runtime_stats": &runtime_stats,
            "server": {
                "name": &server.name,
                "interface": &server.interface,
                "address": server.address.to_string(),
                "subnet": server.subnet.to_string(),
                "v6_address": server.v6_address.map(|address| address.to_string()),
                "v6_subnet": server.v6_subnet.map(|subnet| subnet.to_string()),
                "tunnel": server.tunnel_mode(),
                "full_tunnel": server.full_tunnel,
                "listen_port": server.listen_port,
                "endpoint": server.endpoint.as_deref(),
                "mtu": server.mtu,
                "dns_zone": server.dns_zone.as_deref(),
                "dns_listen": server.dns_listen_addrs(),
                "public_key": server.public_key_b64()?,
            },
            "peers": peers.len(),
            "dns_records": records.len(),
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    println!("Server {} ({})", server.name, server.interface);
    println!(
        "  address:    {}/{}",
        server.address,
        server.subnet.prefix_len()
    );
    if let (Some(v6_address), Some(v6_subnet)) = (server.v6_address, server.v6_subnet) {
        println!(
            "  ipv6:       {v6_address}/{} (subnet {v6_subnet})",
            v6_subnet.prefix_len()
        );
    }
    println!("  tunnel:     {}", server.tunnel_mode());
    println!("  udp:        :{}", server.listen_port);
    println!(
        "  endpoint:   {}",
        server.endpoint.as_deref().unwrap_or("(not set)")
    );
    println!("  mtu:        {}", server.mtu);
    println!("  peers:      {}", peers.len());
    println!("  dns:        {} records", records.len());
    println!(
        "  zone:       {}",
        server.dns_zone.as_deref().unwrap_or("(none)")
    );
    println!("  dns listen: {}", server.dns_listen_addrs().join(", "));
    println!("  database:   {}", db_path.display());
    println!(
        "Runtime: {}",
        if running { "running" } else { "not running" }
    );

    if let Some(stats) = &runtime_stats {
        println!("  pid:        {}", stats["pid"].as_u64().unwrap_or(0));
        println!("  uptime:     {}s", stats["uptime_s"].as_u64().unwrap_or(0));
        if let Some(peers) = stats["peers"].as_array() {
            for peer in peers {
                let key = peer["public_key"].as_str().unwrap_or("");
                let key = key.get(..12).unwrap_or(key);
                let handshake = peer["last_handshake"]
                    .as_u64()
                    .map(util::format_age_epoch)
                    .unwrap_or_else(|| "never".into());
                let rx = util::format_bytes(peer["rx_bytes"].as_u64().unwrap_or(0));
                let tx = util::format_bytes(peer["tx_bytes"].as_u64().unwrap_or(0));
                println!("  peer {key} handshake {handshake} rx {rx} tx {tx}");
            }
        }
    }
    Ok(())
}
