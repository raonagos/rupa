use std::path::Path;

use anyhow::Result;
use serde_json::json;

use crate::{control, db};

pub fn run(db_path: &Path, json_output: bool) -> Result<()> {
    let conn = db::open(db_path)?;
    let server = db::load_server(&conn)?;
    let peers = db::list_peers(&conn, server.id)?;
    let records = db::list_dns_records(&conn, server.id)?;
    let running = control::is_running();

    if json_output {
        let value = json!({
            "db": db_path.display().to_string(),
            "runtime": running,
            "server": {
                "name": &server.name,
                "interface": &server.interface,
                "address": server.address.to_string(),
                "subnet": server.subnet.to_string(),
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
    Ok(())
}
