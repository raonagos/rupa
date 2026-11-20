//! Full-tunnel NAT and forwarding setup.
//!
//! All operations are best effort at the call site: a missing `iptables` or a
//! kernel without the conntrack match must never keep the WireGuard tunnel from
//! coming up.

use std::fs;
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::model::Server;

const ROUTE_TABLE: &str = "/proc/net/route";
const IPV4_FORWARD: &str = "/proc/sys/net/ipv4/ip_forward";
const IPV6_FORWARD: &str = "/proc/sys/net/ipv6/conf/all/forwarding";

/// Enable forwarding and install the NAT/firewall rules for a full tunnel.
pub fn apply(server: &Server) -> Result<()> {
    if !server.full_tunnel {
        return Ok(());
    }
    let wan = detect_wan_interface()?;
    enable_forwarding()?;

    let networks = match server.v6_subnet {
        Some(v6_subnet) => format!("{} and {v6_subnet}", server.subnet),
        None => server.subnet.to_string(),
    };
    let interface = server.interface.as_str();
    let v4_subnet = server.subnet.to_string();
    install_rule(
        "iptables",
        Some("nat"),
        "POSTROUTING",
        &["-s", &v4_subnet, "-o", &wan, "-j", "MASQUERADE"],
    )?;
    install_rule(
        "iptables",
        None,
        "FORWARD",
        &["-i", interface, "-j", "ACCEPT"],
    )?;
    install_established("iptables", interface)?;

    if let Some(v6_subnet) = server.v6_subnet {
        let v6_subnet = v6_subnet.to_string();
        install_rule(
            "ip6tables",
            Some("nat"),
            "POSTROUTING",
            &["-s", &v6_subnet, "-o", &wan, "-j", "MASQUERADE"],
        )?;
        install_rule(
            "ip6tables",
            None,
            "FORWARD",
            &["-i", interface, "-j", "ACCEPT"],
        )?;
        install_established("ip6tables", interface)?;
    }

    eprintln!(
        "rupa: full tunnel: NAT {networks} via {wan} on {}",
        server.interface
    );
    Ok(())
}

/// Remove the rules installed by [`apply`]. Best effort: missing rules or a
/// missing table are not an error on shutdown.
pub fn remove(server: &Server) -> Result<()> {
    if !server.full_tunnel {
        return Ok(());
    }
    let wan = match detect_wan_interface() {
        Ok(wan) => wan,
        Err(err) => {
            eprintln!("rupa: warning: full tunnel: not removing NAT rules: {err:#}");
            return Ok(());
        }
    };

    let interface = server.interface.as_str();
    let v4_subnet = server.subnet.to_string();
    delete_rule(
        "iptables",
        Some("nat"),
        "POSTROUTING",
        &["-s", &v4_subnet, "-o", &wan, "-j", "MASQUERADE"],
    );
    delete_rule(
        "iptables",
        None,
        "FORWARD",
        &["-i", interface, "-j", "ACCEPT"],
    );
    delete_established("iptables", interface);

    if let Some(v6_subnet) = server.v6_subnet {
        let v6_subnet = v6_subnet.to_string();
        delete_rule(
            "ip6tables",
            Some("nat"),
            "POSTROUTING",
            &["-s", &v6_subnet, "-o", &wan, "-j", "MASQUERADE"],
        );
        delete_rule(
            "ip6tables",
            None,
            "FORWARD",
            &["-i", interface, "-j", "ACCEPT"],
        );
        delete_established("ip6tables", interface);
    }
    Ok(())
}

/// Read the interface of the default route (`Destination` 00000000) from
/// `/proc/net/route`; no `ip route` binary is needed.
fn detect_wan_interface() -> Result<String> {
    let contents =
        fs::read_to_string(ROUTE_TABLE).with_context(|| format!("failed to read {ROUTE_TABLE}"))?;
    default_route_interface(&contents).with_context(|| format!("no default route in {ROUTE_TABLE}"))
}

fn default_route_interface(contents: &str) -> Option<String> {
    for line in contents.lines().skip(1) {
        let mut fields = line.split_whitespace();
        let Some(interface) = fields.next() else {
            continue;
        };
        let Some(destination) = fields.next() else {
            continue;
        };
        if destination == "00000000" && interface != "lo" {
            return Some(interface.to_string());
        }
    }
    None
}

fn enable_forwarding() -> Result<()> {
    for path in [IPV4_FORWARD, IPV6_FORWARD] {
        fs::write(path, "1").with_context(|| format!("failed to enable forwarding via {path}"))?;
    }
    Ok(())
}

/// Add a rule unless an identical one is already installed. `-C` keeps
/// `rupa up` idempotent across restarts.
fn install_rule(binary: &str, table: Option<&str>, chain: &str, rule: &[&str]) -> Result<()> {
    if rule_exists(binary, table, chain, rule)? {
        return Ok(());
    }
    run_rule(binary, table, "-A", chain, rule).with_context(|| {
        format!(
            "failed to add the {binary} {chain} rule ({})",
            rule.join(" ")
        )
    })
}

fn install_established(binary: &str, interface: &str) -> Result<()> {
    let conntrack = [
        "-o",
        interface,
        "-m",
        "conntrack",
        "--ctstate",
        "RELATED,ESTABLISHED",
        "-j",
        "ACCEPT",
    ];
    let state = [
        "-o",
        interface,
        "-m",
        "state",
        "--state",
        "RELATED,ESTABLISHED",
        "-j",
        "ACCEPT",
    ];
    if rule_exists(binary, None, "FORWARD", &conntrack)?
        || rule_exists(binary, None, "FORWARD", &state)?
    {
        return Ok(());
    }
    if run_rule(binary, None, "-A", "FORWARD", &conntrack).is_ok() {
        return Ok(());
    }
    run_rule(binary, None, "-A", "FORWARD", &state)
        .with_context(|| format!("failed to add the {binary} FORWARD established/related rule"))
}

fn delete_rule(binary: &str, table: Option<&str>, chain: &str, rule: &[&str]) {
    let _ = run_rule(binary, table, "-D", chain, rule);
}

fn delete_established(binary: &str, interface: &str) {
    delete_rule(
        binary,
        None,
        "FORWARD",
        &[
            "-o",
            interface,
            "-m",
            "conntrack",
            "--ctstate",
            "RELATED,ESTABLISHED",
            "-j",
            "ACCEPT",
        ],
    );
    delete_rule(
        binary,
        None,
        "FORWARD",
        &[
            "-o",
            interface,
            "-m",
            "state",
            "--state",
            "RELATED,ESTABLISHED",
            "-j",
            "ACCEPT",
        ],
    );
}

fn rule_exists(binary: &str, table: Option<&str>, chain: &str, rule: &[&str]) -> Result<bool> {
    let output = rule_command(binary, table, "-C", chain, rule)
        .output()
        .with_context(|| format!("failed to run {binary}"))?;
    Ok(output.status.success())
}

fn run_rule(
    binary: &str,
    table: Option<&str>,
    operation: &str,
    chain: &str,
    rule: &[&str],
) -> Result<()> {
    let output = rule_command(binary, table, operation, chain, rule)
        .output()
        .with_context(|| format!("failed to run {binary}"))?;
    if !output.status.success() {
        bail!(
            "{binary} {operation} {chain} {} failed: {}",
            rule.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn rule_command(
    binary: &str,
    table: Option<&str>,
    operation: &str,
    chain: &str,
    rule: &[&str],
) -> Command {
    let mut command = Command::new(binary);
    if let Some(table) = table {
        command.args(["-t", table]);
    }
    command.args([operation, chain]);
    command.args(rule);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTE: &str = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
lo\t00000000\t00000000\t0001\t0\t0\t0\t00000000\t0\t0\t0
eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0
eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0
";

    #[test]
    fn finds_the_default_route_interface() {
        assert_eq!(default_route_interface(ROUTE).as_deref(), Some("eth0"));
        assert_eq!(default_route_interface("Iface\tDestination\n"), None);
        assert_eq!(
            default_route_interface("Iface\tDestination\neth0\t00000000\t00000000\t0003\n")
                .as_deref(),
            Some("eth0")
        );
    }

    #[test]
    fn ignores_loopback_default_routes() {
        let route = "Iface\tDestination\nlo\t00000000\t00000000\t0001\n";
        assert_eq!(default_route_interface(route), None);
    }
}
