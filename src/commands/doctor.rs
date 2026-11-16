use std::net::{ToSocketAddrs, UdpSocket};
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};

use crate::{control, db};

pub fn run(db_path: &Path) -> Result<()> {
    let mut failed = false;

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

                    match UdpSocket::bind(("0.0.0.0", server.listen_port)) {
                        Ok(_) => ok(&format!("udp/{} is free", server.listen_port)),
                        Err(err) => warn(&format!(
                            "udp/{} is not bindable (server already running?): {err}",
                            server.listen_port
                        )),
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
                            warn(&format!(
                                "interface {} already exists (server running?)",
                                server.interface
                            ));
                        } else {
                            ok(&format!("interface {} is free", server.interface));
                        }
                    }

                    ok(&format!(
                        "DNS listeners: {}",
                        server.dns_listen_addrs().join(", ")
                    ));
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

    if control::is_running() {
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
