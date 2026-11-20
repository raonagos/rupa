use std::path::Path;

use anyhow::{bail, Result};

use crate::cli::InitArgs;
use crate::db::{self, NewServer};
use crate::ip;
use crate::keys;
use crate::util;

pub fn run(db_path: &Path, args: InitArgs) -> Result<()> {
    let subnet = ip::parse_subnet(&args.subnet)?;
    let address = match args.address.as_deref() {
        Some(value) => ip::parse_server_address(value, &subnet)?,
        None => ip::default_server_address(&subnet),
    };
    ip::validate_server_address(address, &subnet)?;

    let v6_subnet = if args.no_ipv6 {
        None
    } else {
        Some(match args.v6_subnet.as_deref() {
            Some(value) => ip::parse_v6_subnet(value)?,
            None => ip::random_ula()?,
        })
    };
    let v6_address = v6_subnet.map(|net| ip::default_v6_address(&net));

    let endpoint = match args.endpoint.as_deref() {
        Some(value) => util::validate_endpoint(value)?,
        None => util::detect_endpoint()?,
    };
    let interface = util::validate_interface(&args.interface)?;
    util::validate_mtu(args.mtu)?;
    if args.port == 0 {
        bail!("listen port must be between 1 and 65535");
    }
    if args.name.trim().is_empty() {
        bail!("server name must not be empty");
    }

    let dns_zone = args
        .dns_zone
        .as_deref()
        .map(util::validate_dns_name)
        .transpose()?;
    let dns_upstreams = util::validate_upstreams(&args.dns_upstreams)?;
    let dns_listen = args
        .dns_listen
        .as_deref()
        .map(util::validate_dns_listen)
        .transpose()?;

    let pair = keys::generate()?;
    let created_at = util::now_rfc3339();

    let conn = db::create(db_path)?;
    let server = NewServer {
        name: args.name.trim().to_string(),
        private_key: pair.private,
        public_key: pair.public,
        listen_port: args.port,
        address,
        subnet,
        v6_subnet,
        v6_address,
        full_tunnel: args.full_tunnel,
        mtu: args.mtu,
        endpoint,
        interface: interface.clone(),
        dns_zone: dns_zone.clone(),
        dns_upstreams,
        dns_listen: dns_listen.clone(),
        created_at,
    };

    if let Err(err) = db::insert_server(&conn, &server) {
        drop(conn);
        let _ = std::fs::remove_file(db_path);
        return Err(err);
    }

    let dns_display = dns_listen.unwrap_or_else(|| format!("{address}:53"));

    println!("Created {}", db_path.display());
    println!("Server {} ({interface})", server.name);
    println!("  address:    {address}/{}", subnet.prefix_len());
    match (v6_address, v6_subnet) {
        (Some(v6_address), Some(v6_subnet)) => println!(
            "  ipv6:       {v6_address}/{} (subnet {v6_subnet})",
            v6_subnet.prefix_len()
        ),
        _ => println!("  ipv6:       disabled"),
    }
    println!(
        "  tunnel:     {}",
        if args.full_tunnel { "full" } else { "split" }
    );
    println!("  udp:        :{}", args.port);
    println!("  endpoint:   {}", server.endpoint);
    println!("  public key: {}", keys::encode(&pair.public));
    match dns_zone {
        Some(zone) => println!("  DNS:        {dns_display} (zone {zone})"),
        None => println!("  DNS:        {dns_display} (no authoritative zone)"),
    }
    if args.full_tunnel {
        println!();
        println!(
            "Full tunnel: the server will NAT the tunnel; forwarding and firewall rules are configured on `rupa up`."
        );
    }
    println!();
    println!("Next:");
    println!("  sudo rupa up");
    println!("  sudo rupa peer add laptop --dns laptop --qr");
    println!();
    println!("If ufw is enabled, allow the VPN:");
    println!("  ufw allow {}/udp", args.port);
    println!("  ufw allow in on {interface}");
    Ok(())
}
