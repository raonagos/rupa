use std::net::{ToSocketAddrs, UdpSocket};
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};

use crate::{control, db};

pub fn run(db_path: &Path) -> Result<()> {
    let mut failed = false;
    let running = control::is_running();

    println!("rupa doctor");
    println!();

    let euid = unsafe { libc::geteuid() };
    if euid == 0 {
        ok("running as root");
    } else {
        warn(&format!(
            "running as uid {euid}; TUN and port 53 need root (or capabilities)"
        ));
    }

    if Path::new("/dev/net/tun").exists() {
        ok("/dev/net/tun is present");
    } else {
        fail("/dev/net/tun is missing");
        failed = true;
    }

    let has_ip = which("ip");
    if has_ip {
        ok("ip is in PATH");
    } else {
        fail("ip is not in PATH");
        failed = true;
    }

    if which("nft") {
        ok("nft is in PATH");
    } else {
        warn("nft is not in PATH (only needed for NAT in 0.2)");
    }

    match db::open(db_path) {
        Ok(conn) => {
            ok(&format!("database {}", db_path.display()));
            match db::load_server(&conn) {
                Ok(server) => {
                    ok(&format!("server {} ({})", server.name, server.interface));

                    match server.endpoint.as_deref() {
                        Some(endpoint) => {
                            ok(&format!("endpoint {endpoint}:{}", server.listen_port));
                            let target = format!("{endpoint}:{}", server.listen_port);
                            match target.to_socket_addrs() {
                                Ok(mut addrs) => {
                                    if addrs.next().is_some() {
                                        ok("endpoint resolves locally");
                                    } else {
                                        warn("endpoint resolved to no address");
                                    }
                                }
                                Err(err) => warn(&format!(
                                    "endpoint does not resolve locally: {err} (clients may still resolve it)"
                                )),
                            }
                        }
                        None => {
                            fail("server has no endpoint; rerun `rupa init --endpoint <host>`");
                            failed = true;
                        }
                    }

                    if running {
                        ok(&format!(
                            "udp/{} is held by the running rupa server",
                            server.listen_port
                        ));
                    } else {
                        match UdpSocket::bind(("0.0.0.0", server.listen_port)) {
                            Ok(_) => ok(&format!("udp/{} is free", server.listen_port)),
                            Err(err) => warn(&format!(
                                "udp/{} is not bindable (another process?): {err}",
                                server.listen_port
                            )),
                        }
                    }

                    if has_ip {
                        let exists = Command::new("ip")
                            .arg("link")
                            .arg("show")
                            .arg(&server.interface)
                            .output()
                            .map(|output| output.status.success())
                            .unwrap_or(false);
                        if exists {
                            if running {
                                ok(&format!(
                                    "interface {} exists (running server)",
                                    server.interface
                                ));
                            } else {
                                warn(&format!(
                                    "interface {} already exists (stale?)",
                                    server.interface
                                ));
                            }
                        } else if running {
                            fail(&format!(
                                "control socket is up but interface {} is missing",
                                server.interface
                            ));
                            failed = true;
                        } else {
                            ok(&format!("interface {} is free", server.interface));
                        }
                    }

                    if let Some(v6_address) = server.v6_address {
                        if has_ip {
                            let configured = Command::new("ip")
                                .args(["-6", "addr", "show", "dev", &server.interface])
                                .output()
                                .map(|output| {
                                    output.status.success()
                                        && String::from_utf8_lossy(&output.stdout)
                                            .contains(&format!("inet6 {v6_address}"))
                                })
                                .unwrap_or(false);
                            if configured {
                                ok(&format!(
                                    "interface {} has IPv6 {v6_address}",
                                    server.interface
                                ));
                            } else if running {
                                fail(&format!(
                                    "interface {} is missing IPv6 {v6_address}",
                                    server.interface
                                ));
                                failed = true;
                            } else {
                                warn(&format!(
                                    "IPv6 {v6_address} is not on {} (server not running)",
                                    server.interface
                                ));
                            }
                        }
                    }

                    if server.full_tunnel {
                        match std::fs::read_to_string("/proc/sys/net/ipv4/ip_forward") {
                            Ok(value) if value.trim() == "1" => {
                                ok("full tunnel: net.ipv4.ip_forward is 1")
                            }
                            Ok(value) => {
                                fail(&format!(
                                    "full tunnel: net.ipv4.ip_forward is {} (expected 1)",
                                    value.trim()
                                ));
                                failed = true;
                            }
                            Err(err) => warn(&format!(
                                "full tunnel: cannot read net.ipv4.ip_forward: {err}"
                            )),
                        }
                    }

                    ok(&format!(
                        "DNS listeners: {}",
                        server.dns_listen_addrs().join(", ")
                    ));

                    if euid == 0 {
                        check_ufw(server.listen_port, &server.interface);
                    }
                }
                Err(err) => {
                    fail(&format!("server: {err:#}"));
                    failed = true;
                }
            }
        }
        Err(err) => {
            fail(&format!("database: {err:#}"));
            failed = true;
        }
    }

    if running {
        ok("control socket is accepting connections");
    } else {
        warn("server is not running (expected before `rupa up`)");
    }

    println!();
    if failed {
        bail!("doctor found blocking problems");
    }
    println!("doctor: no blocking problem found");
    Ok(())
}

fn check_ufw(port: u16, interface: &str) {
    let output = match Command::new("ufw").arg("status").output() {
        Ok(output) => output,
        Err(_) => return,
    };
    if !output.status.success() {
        return;
    }
    let status = String::from_utf8_lossy(&output.stdout);
    if !status.contains("Status: active") {
        ok("ufw is not active");
        return;
    }

    let (port_ok, interface_ok) = ufw_rules(&status, port, interface);
    match (port_ok, interface_ok) {
        (true, true) => ok(&format!("ufw allows {port}/udp and {interface}")),
        _ => {
            warn("ufw is active; missing rules:");
            if !port_ok {
                println!("         ufw allow {port}/udp");
            }
            if !interface_ok {
                println!("         ufw allow in on {interface}");
            }
        }
    }
}

/// Return (port UDP allowed, interface allowed) from `ufw status` output.
fn ufw_rules(status: &str, port: u16, interface: &str) -> (bool, bool) {
    let port_rule = format!("{port}/udp");
    let interface_rule = format!("on {interface}");
    let mut port_ok = false;
    let mut interface_ok = false;

    for line in status.lines() {
        if !line.contains("ALLOW") {
            continue;
        }
        if line.trim_start().starts_with(&port_rule) {
            port_ok = true;
        }
        if line.contains(&interface_rule) {
            interface_ok = true;
        }
    }
    (port_ok, interface_ok)
}

fn ok(message: &str) {
    println!("[OK]   {message}");
}

fn warn(message: &str) {
    println!("[WARN] {message}");
}

fn fail(message: &str) {
    println!("[FAIL] {message}");
}

fn which(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|directory| directory.join(name).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::ufw_rules;

    #[test]
    fn detects_allowed_port_and_interface() {
        let status = "\
Status: active

To                         Action      From
--                         ------      ----
51820/udp                  ALLOW       Anywhere
Anywhere on rupa0          ALLOW       Anywhere
51820/udp (v6)             ALLOW       Anywhere (v6)
Anywhere (v6) on rupa0     ALLOW       Anywhere (v6)
";
        assert_eq!(ufw_rules(status, 51820, "rupa0"), (true, true));
        assert_eq!(ufw_rules(status, 51821, "rupa0"), (false, true));
        assert_eq!(ufw_rules(status, 51820, "rupa1"), (true, false));
    }

    #[test]
    fn deny_and_limit_are_not_allow() {
        let status = "\
Status: active
51820/udp                  DENY        Anywhere
Anywhere on rupa0          LIMIT       Anywhere
";
        assert_eq!(ufw_rules(status, 51820, "rupa0"), (false, false));
    }
}
