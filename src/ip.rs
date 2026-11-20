use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr};

use anyhow::{bail, Context, Result};
use ipnet::{Ipv4Net, Ipv6Net};

pub fn parse_subnet(value: &str) -> Result<Ipv4Net> {
    let net: Ipv4Net = value
        .trim()
        .parse()
        .with_context(|| format!("invalid subnet '{value}' (expected e.g. 10.8.0.0/24)"))?;
    if net.prefix_len() > 30 {
        bail!("subnet {net} is too small: at least one peer address is needed");
    }
    Ok(net.trunc())
}

pub fn default_server_address(subnet: &Ipv4Net) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(subnet.network()) + 1)
}

pub fn parse_server_address(value: &str, subnet: &Ipv4Net) -> Result<Ipv4Addr> {
    let address: Ipv4Addr = value
        .trim()
        .parse()
        .with_context(|| format!("invalid server address '{value}'"))?;
    validate_server_address(address, subnet)?;
    Ok(address)
}

pub fn validate_server_address(address: Ipv4Addr, subnet: &Ipv4Net) -> Result<()> {
    if !subnet.contains(&address) {
        bail!("server address {address} is outside subnet {subnet}");
    }
    if address == subnet.network() || address == subnet.broadcast() {
        bail!("server address {address} is the network or broadcast address of {subnet}");
    }
    Ok(())
}

/// Return the first free host address in `subnet`, skipping the server address.
pub fn allocate(subnet: &Ipv4Net, server: Ipv4Addr, used: &HashSet<Ipv4Addr>) -> Result<Ipv4Addr> {
    let first = u32::from(subnet.network()) + 1;
    let last = u32::from(subnet.broadcast());
    let mut current = first;
    while current < last {
        let address = Ipv4Addr::from(current);
        if address != server && !used.contains(&address) {
            return Ok(address);
        }
        current += 1;
    }
    bail!("subnet {subnet} has no free peer address")
}

/// Parse an IPv6 tunnel subnet: a unique local /64 inside `fd00::/8`.
pub fn parse_v6_subnet(value: &str) -> Result<Ipv6Net> {
    let net: Ipv6Net = value.trim().parse().with_context(|| {
        format!("invalid IPv6 subnet '{value}' (expected e.g. fd00:1234:5678::/64)")
    })?;
    if net.prefix_len() != 64 {
        bail!(
            "IPv6 subnet {net} must be a /64 (got /{}): the host part holds the peer addresses",
            net.prefix_len()
        );
    }
    let net = net.trunc();
    if net.network().octets()[0] != 0xfd {
        bail!("IPv6 subnet {net} is not a unique local address: it must be inside fd00::/8");
    }
    Ok(net)
}

/// The `::1` address of an IPv6 subnet, used by the server.
pub fn default_v6_address(subnet: &Ipv6Net) -> Ipv6Addr {
    Ipv6Addr::from(u128::from(subnet.network()) + 1)
}

/// Return the first free host address after `::1` in `subnet`, skipping the
/// server address.
pub fn allocate_v6(
    subnet: &Ipv6Net,
    server: Ipv6Addr,
    used: &HashSet<Ipv6Addr>,
) -> Result<Ipv6Addr> {
    if !subnet.contains(&server) {
        bail!("server IPv6 address {server} is outside subnet {subnet}");
    }
    let first = u128::from(subnet.network()) + 2;
    let last = u128::from(subnet.broadcast());
    let mut current = first;
    while current < last {
        let address = Ipv6Addr::from(current);
        if address != server && !used.contains(&address) {
            return Ok(address);
        }
        current += 1;
    }
    bail!("subnet {subnet} has no free peer address")
}

/// Generate a random RFC 4193 unique local /64: `fd` + 40 random global-ID
/// bits + 16 random subnet-ID bits.
pub fn random_ula() -> Result<Ipv6Net> {
    let mut bytes = [0u8; 16];
    bytes[0] = 0xfd;
    getrandom::getrandom(&mut bytes[1..8])
        .map_err(|err| anyhow::anyhow!("failed to read random bytes for the IPv6 subnet: {err}"))?;
    Ipv6Net::new(Ipv6Addr::from(bytes), 64).context("failed to build the random IPv6 subnet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_the_first_free_host() {
        let subnet: Ipv4Net = "10.8.0.0/24".parse().unwrap();
        let server = Ipv4Addr::new(10, 8, 0, 1);
        let mut used = HashSet::new();
        assert_eq!(
            allocate(&subnet, server, &used).unwrap(),
            Ipv4Addr::new(10, 8, 0, 2)
        );
        used.insert(Ipv4Addr::new(10, 8, 0, 2));
        used.insert(Ipv4Addr::new(10, 8, 0, 3));
        assert_eq!(
            allocate(&subnet, server, &used).unwrap(),
            Ipv4Addr::new(10, 8, 0, 4)
        );
    }

    #[test]
    fn skips_the_server_address() {
        let subnet: Ipv4Net = "10.8.0.0/30".parse().unwrap();
        let server = Ipv4Addr::new(10, 8, 0, 1);
        let used = HashSet::new();
        assert_eq!(
            allocate(&subnet, server, &used).unwrap(),
            Ipv4Addr::new(10, 8, 0, 2)
        );
        let mut used = HashSet::new();
        used.insert(Ipv4Addr::new(10, 8, 0, 2));
        assert!(allocate(&subnet, server, &used).is_err());
    }

    #[test]
    fn normalizes_and_rejects_tiny_subnets() {
        let net = parse_subnet("10.9.0.5/24").unwrap();
        assert_eq!(net.to_string(), "10.9.0.0/24");
        assert!(parse_subnet("10.9.0.0/31").is_err());
        assert!(parse_subnet("10.9.0.0/32").is_err());
    }

    #[test]
    fn validates_the_server_address() {
        let subnet: Ipv4Net = "10.8.0.0/24".parse().unwrap();
        assert!(validate_server_address(Ipv4Addr::new(10, 8, 0, 1), &subnet).is_ok());
        assert!(validate_server_address(Ipv4Addr::new(10, 8, 0, 0), &subnet).is_err());
        assert!(validate_server_address(Ipv4Addr::new(10, 8, 0, 255), &subnet).is_err());
        assert!(validate_server_address(Ipv4Addr::new(10, 9, 0, 1), &subnet).is_err());
    }

    #[test]
    fn parses_only_ula_slash_64_subnets() {
        let net = parse_v6_subnet("fd12:3456:789a::/64").unwrap();
        assert_eq!(net.to_string(), "fd12:3456:789a::/64");

        let truncated = parse_v6_subnet("fd12:3456:789a:abcd::1/64").unwrap();
        assert_eq!(truncated.to_string(), "fd12:3456:789a:abcd::/64");

        assert!(parse_v6_subnet("2001:db8::/64").is_err());
        assert!(parse_v6_subnet("fd12:3456:789a::/48").is_err());
        assert!(parse_v6_subnet("fc12:3456:789a::/64").is_err());
        assert!(parse_v6_subnet("10.8.0.0/24").is_err());
    }

    #[test]
    fn defaults_to_the_first_host_of_the_ula() {
        let subnet = parse_v6_subnet("fd12:3456:789a::/64").unwrap();
        assert_eq!(
            default_v6_address(&subnet),
            "fd12:3456:789a::1".parse::<Ipv6Addr>().unwrap()
        );
    }

    #[test]
    fn allocates_the_first_free_ipv6_host() {
        let subnet = parse_v6_subnet("fd12:3456:789a::/64").unwrap();
        let server = default_v6_address(&subnet);
        let mut used = HashSet::new();
        assert_eq!(
            allocate_v6(&subnet, server, &used).unwrap(),
            "fd12:3456:789a::2".parse::<Ipv6Addr>().unwrap()
        );
        used.insert("fd12:3456:789a::2".parse().unwrap());
        used.insert("fd12:3456:789a::3".parse().unwrap());
        assert_eq!(
            allocate_v6(&subnet, server, &used).unwrap(),
            "fd12:3456:789a::4".parse::<Ipv6Addr>().unwrap()
        );

        let outside = "fd12:3456:789b::1".parse().unwrap();
        assert!(allocate_v6(&subnet, outside, &HashSet::new()).is_err());
    }

    #[test]
    fn generates_a_random_ula_slash_64() {
        let net = random_ula().unwrap();
        assert_eq!(net.prefix_len(), 64);
        assert_eq!(net.network().octets()[0], 0xfd);
        assert_eq!(net.to_string(), net.trunc().to_string());
        // 48 random bits make accidental reuse vanishingly unlikely.
        assert_ne!(net, random_ula().unwrap());
    }
}
