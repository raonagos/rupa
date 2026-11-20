use std::net::{Ipv4Addr, Ipv6Addr};

use anyhow::{bail, Context, Result};
use ipnet::{Ipv4Net, Ipv6Net};

use crate::keys;

#[derive(Debug, Clone)]
pub struct Server {
    pub id: i64,
    pub name: String,
    pub private_key: Vec<u8>,
    pub public_key: Vec<u8>,
    pub listen_port: u16,
    pub address: Ipv4Addr,
    pub subnet: Ipv4Net,
    pub v6_subnet: Option<Ipv6Net>,
    pub v6_address: Option<Ipv6Addr>,
    pub full_tunnel: bool,
    pub mtu: u16,
    pub endpoint: Option<String>,
    pub interface: String,
    pub dns_zone: Option<String>,
    pub dns_upstreams: String,
    pub dns_listen: Option<String>,
    pub created_at: String,
}

impl Server {
    pub fn private_key_b64(&self) -> Result<String> {
        key_b64(&self.private_key)
    }

    pub fn public_key_b64(&self) -> Result<String> {
        key_b64(&self.public_key)
    }

    /// Whether the tunnel carries IPv6.
    pub fn has_ipv6(&self) -> bool {
        self.v6_subnet.is_some() && self.v6_address.is_some()
    }

    /// `full` routes all client traffic through the server, `split` only the
    /// tunnel subnets.
    pub fn tunnel_mode(&self) -> &'static str {
        if self.full_tunnel {
            "full"
        } else {
            "split"
        }
    }

    /// Configured DNS listeners, or `<tunnel address>:53` by default.
    pub fn dns_listen_addrs(&self) -> Vec<String> {
        match self.dns_listen.as_deref() {
            Some(value) if !value.trim().is_empty() => value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
            _ => vec![format!("{}:53", self.address)],
        }
    }
}

#[derive(Debug, Clone)]
pub struct Peer {
    pub id: i64,
    pub server_id: i64,
    pub name: String,
    pub private_key: Vec<u8>,
    pub public_key: Vec<u8>,
    pub address: Ipv4Addr,
    pub v6_address: Option<Ipv6Addr>,
    pub created_at: String,
    pub enabled: bool,
}

impl Peer {
    pub fn private_key_b64(&self) -> Result<String> {
        key_b64(&self.private_key)
    }

    pub fn public_key_b64(&self) -> Result<String> {
        key_b64(&self.public_key)
    }
}

#[derive(Debug, Clone)]
pub struct PeerWithDns {
    pub peer: Peer,
    pub dns_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsSource {
    Manual,
    Uping,
    Peer,
}

impl DnsSource {
    pub fn as_str(self) -> &'static str {
        match self {
            DnsSource::Manual => "manual",
            DnsSource::Uping => "uping",
            DnsSource::Peer => "peer",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "manual" => Ok(DnsSource::Manual),
            "uping" => Ok(DnsSource::Uping),
            "peer" => Ok(DnsSource::Peer),
            other => bail!("unknown DNS record source '{other}'"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DnsRecord {
    pub id: i64,
    pub server_id: i64,
    pub name: String,
    pub address: Ipv4Addr,
    pub source: DnsSource,
    pub peer_id: Option<i64>,
    pub updated_at: String,
}

fn key_b64(key: &[u8]) -> Result<String> {
    if key.len() != 32 {
        bail!("stored key has invalid length {}", key.len());
    }
    Ok(keys::encode(key))
}

pub fn parse_ip(value: &str, context: &str) -> Result<Ipv4Addr> {
    value
        .parse()
        .with_context(|| format!("invalid IPv4 address '{value}' in {context}"))
}
