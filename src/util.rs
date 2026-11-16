use std::io::{self, IsTerminal, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};

/// Normalize and validate a peer name.
///
/// Peer names are lowercase, start with a letter or digit, contain only
/// lowercase letters, digits and hyphens, and are at most 32 characters.
pub fn validate_peer_name(input: &str) -> Result<String> {
    let name = input.trim().to_ascii_lowercase();
    if name.is_empty() || name.len() > 32 {
        bail!("peer name must be 1..=32 characters");
    }
    if name.starts_with('-') || name.ends_with('-') {
        bail!("peer name must not start or end with a hyphen");
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        bail!("peer name may only contain lowercase letters, digits and hyphens");
    }
    Ok(name)
}

/// Normalize and validate a fully qualified DNS name.
pub fn validate_dns_name(input: &str) -> Result<String> {
    let name = input.trim().trim_end_matches('.').to_ascii_lowercase();
    if name.is_empty() || name.len() > 253 {
        bail!("DNS name must be 1..=253 characters");
    }
    if !name.contains('.') {
        bail!(
            "DNS name '{name}' is not fully qualified (expected e.g. site1.internal.example.com)"
        );
    }
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 {
            bail!("DNS name '{name}' has an empty or oversized label");
        }
        if label.starts_with('-') || label.ends_with('-') {
            bail!("DNS name '{name}' has a label starting or ending with a hyphen");
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            bail!("DNS name '{name}' contains an invalid character");
        }
    }
    Ok(name)
}

/// Validate the public endpoint: an IPv4 address or a hostname without port.
pub fn validate_endpoint(input: &str) -> Result<String> {
    let value = input.trim().trim_end_matches('.');
    if value.is_empty() {
        bail!("endpoint must not be empty");
    }
    if value.contains(':') {
        bail!("endpoint must not contain a port; use --port for the UDP port");
    }
    if let Ok(ip) = value.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => Ok(v4.to_string()),
            IpAddr::V6(_) => bail!("IPv6 endpoints are not supported in 0.1.0"),
        };
    }
    if value.len() > 253 {
        bail!("endpoint is too long");
    }
    for label in value.split('.') {
        if label.is_empty() || label.len() > 63 {
            bail!("endpoint '{value}' has an empty or oversized label");
        }
        if label.starts_with('-') || label.ends_with('-') {
            bail!("endpoint '{value}' has a label starting or ending with a hyphen");
        }
        if !label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            bail!("endpoint '{value}' contains an invalid character");
        }
    }
    Ok(value.to_ascii_lowercase())
}

/// Validate a Linux interface name (IFNAMSIZ is 16, so 15 characters max).
pub fn validate_interface(input: &str) -> Result<String> {
    let name = input.trim();
    if name.is_empty() || name.len() > 15 {
        bail!("interface name must be 1..=15 characters");
    }
    if name == "." || name == ".." {
        bail!("invalid interface name '{name}'");
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        bail!("interface name may only contain letters, digits, '_', '-' and '.'");
    }
    Ok(name.to_string())
}

pub fn validate_mtu(mtu: u16) -> Result<()> {
    if !(1280..=1500).contains(&mtu) {
        bail!("MTU must be between 1280 and 1500");
    }
    Ok(())
}

pub fn validate_upstreams(input: &str) -> Result<String> {
    let mut out: Vec<String> = Vec::new();
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let ip: Ipv4Addr = part
            .parse()
            .with_context(|| format!("invalid upstream resolver '{part}'"))?;
        let text = ip.to_string();
        if !out.contains(&text) {
            out.push(text);
        }
    }
    if out.is_empty() {
        bail!("at least one DNS upstream is required");
    }
    Ok(out.join(","))
}

pub fn validate_dns_listen(input: &str) -> Result<String> {
    let mut out: Vec<String> = Vec::new();
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let addr: SocketAddr = part
            .parse()
            .with_context(|| format!("invalid DNS listen address '{part}' (expected IP:port)"))?;
        match addr {
            SocketAddr::V4(_) => {
                let text = addr.to_string();
                if !out.contains(&text) {
                    out.push(text);
                }
            }
            SocketAddr::V6(_) => bail!("IPv6 DNS listeners are not supported in 0.1.0"),
        }
    }
    if out.is_empty() {
        bail!("at least one DNS listen address is required");
    }
    Ok(out.join(","))
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub fn short_date(rfc3339: &str) -> String {
    DateTime::parse_from_rfc3339(rfc3339)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| rfc3339.chars().take(10).collect())
}

/// Ask for confirmation. Refuses to guess when stdin is not a terminal.
pub fn confirm(prompt: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        bail!("refusing to continue without --yes in a non-interactive shell");
    }
    eprint!("{prompt} [y/N] ");
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Detect the source address used to reach the public internet.
///
/// This is only a convenience for `rupa init --endpoint`; behind NAT it
/// returns a private address and the operator must pass the real endpoint.
pub fn detect_endpoint() -> Result<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").context("failed to open a UDP socket")?;
    socket
        .connect("1.1.1.1:80")
        .context("failed to determine the outbound route")?;
    let ip = match socket
        .local_addr()
        .context("failed to read the local address")?
        .ip()
    {
        IpAddr::V4(v4) => v4,
        IpAddr::V6(_) => bail!("could not detect an IPv4 endpoint"),
    };
    if ip.is_unspecified() || ip.is_loopback() {
        bail!("could not detect a usable endpoint; pass --endpoint");
    }
    if ip.is_private() {
        eprintln!(
            "note: detected private address {ip}; pass --endpoint if clients connect over the internet"
        );
    }
    Ok(ip.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_names_are_normalized() {
        assert_eq!(validate_peer_name(" Alice ").unwrap(), "alice");
        assert!(validate_peer_name("").is_err());
        assert!(validate_peer_name("bad_name").is_err());
        assert!(validate_peer_name("-bad").is_err());
        assert!(validate_peer_name("bad-").is_err());
    }

    #[test]
    fn dns_names_require_a_dot() {
        assert_eq!(
            validate_dns_name("Site1.Internal.Example.COM.").unwrap(),
            "site1.internal.example.com"
        );
        assert!(validate_dns_name("nas").is_err());
        assert!(validate_dns_name("bad..example.com").is_err());
    }

    #[test]
    fn endpoints_reject_ports_and_ipv6() {
        assert_eq!(
            validate_endpoint("VPN.Example.com.").unwrap(),
            "vpn.example.com"
        );
        assert_eq!(validate_endpoint("203.0.113.10").unwrap(), "203.0.113.10");
        assert!(validate_endpoint("vpn.example.com:51820").is_err());
        assert!(validate_endpoint("::1").is_err());
    }

    #[test]
    fn dns_listen_requires_ip_port() {
        assert_eq!(
            validate_dns_listen("10.8.0.1:53, 127.0.0.1:53").unwrap(),
            "10.8.0.1:53,127.0.0.1:53"
        );
        assert!(validate_dns_listen("10.8.0.1").is_err());
    }
}
