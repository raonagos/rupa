//! Serve rupa's DNS records without the WireGuard runtime.
//!
//! Useful for debugging the resolver on a high port:
//!
//! ```sh
//! cargo run --example dns-serve -- /var/lib/rupa/rupa.db 127.0.0.1:5353
//! dig @127.0.0.1 -p 5353 site1.internal.example.com A
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};

use rupa::db;
use rupa::dns::{DnsServer, DnsTable};

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let db_path = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "/var/lib/rupa/rupa.db".into()),
    );
    let listen: SocketAddr = args
        .next()
        .unwrap_or_else(|| "127.0.0.1:5353".into())
        .parse()
        .context("invalid listen address")?;

    let conn = db::open(&db_path)?;
    let server = db::load_server(&conn)?;
    let table = Arc::new(RwLock::new(DnsTable::from_db(&conn, &server)?));
    let dns = DnsServer::bind(table, &[listen]).await?;

    println!(
        "rupa DNS on {listen} (zone {}, {} records)",
        server.dns_zone.as_deref().unwrap_or("(none)"),
        rupa::db::list_dns_records(&conn, server.id)?.len()
    );
    dns.run().await
}
