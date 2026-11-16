use std::collections::HashSet;
use std::net::Ipv4Addr;

use anyhow::{bail, Context, Result};
use ipnet::Ipv4Net;

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
}
