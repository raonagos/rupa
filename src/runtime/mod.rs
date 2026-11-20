use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;

use wireguard::configuration::{Configuration, WireGuardConfig};
use wireguard::platform::plt;
use wireguard::platform::tun::PlatformTun;
use wireguard::x25519_dalek::{PublicKey, StaticSecret};
use wireguard::WireGuard;

use crate::control::{self, DEFAULT_SOCKET};
use crate::db;
use crate::dns::{DnsServer, DnsTable, SharedTable};
use crate::keys;
use crate::model::{PeerWithDns, Server};

pub fn run_blocking(db_path: &Path) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build the tokio runtime")?;
    runtime.block_on(run(db_path))
}

pub fn request_down() -> Result<Value> {
    control::request("shutdown")
}

pub fn request_reload() -> Result<Value> {
    control::request("reload")
}

async fn run(db_path: &Path) -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        bail!("`rupa up` needs root: TUN and port 53 require CAP_NET_ADMIN/CAP_NET_BIND_SERVICE");
    }
    if !Path::new("/dev/net/tun").exists() {
        bail!("/dev/net/tun is missing; load the tun module or run on a real Linux host");
    }

    let conn = db::open(db_path)?;
    let server = db::load_server(&conn)?;
    if control::is_running() {
        bail!("another rupa server is already running on {DEFAULT_SOCKET}");
    }

    let (mut readers, writer, _status) = <plt::Tun as PlatformTun>::create(&server.interface)
        .with_context(|| {
            format!(
                "failed to create TUN {} (CAP_NET_ADMIN, or the interface already exists)",
                server.interface
            )
        })?;
    let link_guard = LinkGuard::new(&server.interface);
    setup_link(&server)?;

    let wg: WireGuard<plt::Tun, plt::UDP> = WireGuard::new(writer);
    while let Some(reader) = readers.pop() {
        wg.add_tun_reader(reader);
    }
    let cfg = WireGuardConfig::new(wg);

    let private = key_array(&server.private_key, "server private key")?;
    cfg.set_private_key(Some(StaticSecret::from(private)));
    cfg.set_listen_port(server.listen_port)
        .context("failed to configure the WireGuard UDP port")?;

    let entries = db::list_peers(&conn, server.id)?;
    let mut runtime_peers = HashMap::new();
    for entry in entries.iter().filter(|entry| entry.peer.enabled) {
        install_peer(&cfg, entry, &mut runtime_peers)?;
    }

    cfg.up(server.mtu as usize)
        .context("failed to start WireGuard (is the UDP port already in use?)")?;

    let table: SharedTable = Arc::new(std::sync::RwLock::new(DnsTable::from_db(&conn, &server)?));
    let listeners = parse_listeners(&server)?;
    let dns = DnsServer::bind(table.clone(), &listeners)
        .await
        .context("failed to bind the DNS resolver")?;
    let dns_handle = tokio::spawn(dns.run());

    let listener_list = listeners
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!(
        "rupa {}: {} up {}/{} mtu {}, udp :{}, {} peer(s), dns {}, endpoint {}",
        env!("CARGO_PKG_VERSION"),
        server.interface,
        server.address,
        server.subnet.prefix_len(),
        server.mtu,
        server.listen_port,
        runtime_peers.len(),
        listener_list,
        server.endpoint.as_deref().unwrap_or("-"),
    );

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let ctx = RuntimeCtx {
        cfg,
        table,
        db_path: db_path.to_path_buf(),
        server: server.clone(),
        peers: Arc::new(Mutex::new(runtime_peers)),
        started: Instant::now(),
        shutdown: shutdown_tx,
    };

    let listener = bind_control_socket()?;
    let socket_guard = SocketGuard;
    let control_ctx = ctx.clone();
    let control_handle = tokio::spawn(control_loop(listener, control_ctx));

    let mut hangup = signal(SignalKind::hangup()).context("failed to install SIGHUP handler")?;
    let mut terminate =
        signal(SignalKind::terminate()).context("failed to install SIGTERM handler")?;
    let mut interrupt =
        signal(SignalKind::interrupt()).context("failed to install SIGINT handler")?;

    loop {
        tokio::select! {
            _ = terminate.recv() => break,
            _ = interrupt.recv() => break,
            _ = hangup.recv() => {
                match reload(&ctx) {
                    Ok(value) => eprintln!("rupa: reload: {value}"),
                    Err(err) => eprintln!("rupa: reload failed: {err:#}"),
                }
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
        }
    }

    control_handle.abort();
    dns_handle.abort();
    drop(socket_guard);
    drop(link_guard);
    eprintln!("rupa: stopped");
    Ok(())
}

#[derive(Clone)]
struct RuntimeCtx {
    cfg: WireGuardConfig<plt::Tun, plt::UDP>,
    table: SharedTable,
    db_path: PathBuf,
    server: Server,
    peers: Arc<Mutex<HashMap<[u8; 32], Ipv4Addr>>>,
    started: Instant,
    shutdown: watch::Sender<bool>,
}

fn install_peer(
    cfg: &WireGuardConfig<plt::Tun, plt::UDP>,
    entry: &PeerWithDns,
    runtime: &mut HashMap<[u8; 32], Ipv4Addr>,
) -> Result<()> {
    let key = key_array(&entry.peer.public_key, "peer public key")?;
    let public_key = PublicKey::from(key);
    if cfg.add_peer(&public_key) {
        cfg.add_allowed_ip(&public_key, IpAddr::V4(entry.peer.address), 32);
    }
    runtime.insert(key, entry.peer.address);
    Ok(())
}

fn reload(ctx: &RuntimeCtx) -> Result<Value> {
    let conn = db::open(&ctx.db_path)?;
    let server = db::load_server(&conn)?;
    let restart_required = restart_fields(&ctx.server, &server);
    let entries = db::list_peers(&conn, server.id)?;

    let mut runtime = ctx
        .peers
        .lock()
        .map_err(|_| anyhow::anyhow!("peer runtime state is poisoned"))?;
    let mut desired: HashMap<[u8; 32], Ipv4Addr> = HashMap::new();
    for entry in entries.iter().filter(|entry| entry.peer.enabled) {
        let key = key_array(&entry.peer.public_key, "peer public key")?;
        let public_key = PublicKey::from(key);
        desired.insert(key, entry.peer.address);
        match runtime.get(&key) {
            Some(address) if *address == entry.peer.address => {}
            Some(_) => {
                ctx.cfg.replace_allowed_ips(&public_key);
                ctx.cfg
                    .add_allowed_ip(&public_key, IpAddr::V4(entry.peer.address), 32);
            }
            None => {
                if ctx.cfg.add_peer(&public_key) {
                    ctx.cfg
                        .add_allowed_ip(&public_key, IpAddr::V4(entry.peer.address), 32);
                }
            }
        }
    }
    for key in runtime.keys().copied().collect::<Vec<_>>() {
        if !desired.contains_key(&key) {
            ctx.cfg.remove_peer(&PublicKey::from(key));
        }
    }
    *runtime = desired;
    drop(runtime);

    let new_table = DnsTable::from_db(&conn, &server)?;
    *ctx.table
        .write()
        .map_err(|_| anyhow::anyhow!("DNS table is poisoned"))? = new_table;

    let dns_records = db::list_dns_records(&conn, server.id)?.len();
    Ok(json!({
        "ok": true,
        "peers": entries.len(),
        "dns_records": dns_records,
        "restart_required": restart_required,
    }))
}

fn restart_fields(old: &Server, new: &Server) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if old.private_key != new.private_key {
        fields.push("private_key");
    }
    if old.listen_port != new.listen_port {
        fields.push("listen_port");
    }
    if old.interface != new.interface {
        fields.push("interface");
    }
    if old.address != new.address {
        fields.push("address");
    }
    if old.subnet != new.subnet {
        fields.push("subnet");
    }
    if old.mtu != new.mtu {
        fields.push("mtu");
    }
    if old.dns_listen != new.dns_listen {
        fields.push("dns_listen");
    }
    fields
}

async fn control_loop(listener: UnixListener, ctx: RuntimeCtx) {
    while let Ok((stream, _)) = listener.accept().await {
        let ctx = ctx.clone();
        tokio::spawn(async move {
            handle_control(stream, ctx).await;
        });
    }
}

async fn handle_control(stream: UnixStream, ctx: RuntimeCtx) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).await.is_err() {
        return;
    }
    let request: Value = serde_json::from_str(line.trim()).unwrap_or_else(|_| json!({}));
    let op = request.get("op").and_then(Value::as_str).unwrap_or("");
    let response = match op {
        "ping" => Ok(ping(&ctx)),
        "stats" => Ok(stats(&ctx)),
        "reload" => reload(&ctx),
        "shutdown" => {
            let _ = ctx.shutdown.send(true);
            Ok(json!({ "ok": true }))
        }
        other => Ok(json!({
            "ok": false,
            "error": format!("unknown operation '{other}'"),
        })),
    };
    let payload = response.unwrap_or_else(|err| {
        json!({
            "ok": false,
            "error": format!("{err:#}"),
        })
    });

    let mut stream = reader.into_inner();
    let _ = stream.write_all(payload.to_string().as_bytes()).await;
    let _ = stream.write_all(b"\n").await;
}

fn ping(ctx: &RuntimeCtx) -> Value {
    json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "pid": std::process::id(),
        "uptime_s": ctx.started.elapsed().as_secs(),
        "endpoint": ctx.server.endpoint.as_deref(),
        "listen_port": ctx.server.listen_port,
        "interface": ctx.server.interface,
    })
}

fn stats(ctx: &RuntimeCtx) -> Value {
    let peers = ctx.cfg.get_peers();
    let peers_json: Vec<Value> = peers
        .iter()
        .map(|state| {
            json!({
                "public_key": keys::encode(&state.public_key.to_bytes()),
                "endpoint": state.endpoint.map(|endpoint| endpoint.to_string()),
                "last_handshake": state.last_handshake_time.map(|(secs, _)| secs),
                "rx_bytes": state.rx_bytes,
                "tx_bytes": state.tx_bytes,
                "allowed_ips": state.allowed_ips.iter()
                    .map(|(ip, mask)| format!("{ip}/{mask}"))
                    .collect::<Vec<_>>(),
                "persistent_keepalive": state.persistent_keepalive_interval,
            })
        })
        .collect();

    json!({
        "ok": true,
        "pid": std::process::id(),
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_s": ctx.started.elapsed().as_secs(),
        "peers": peers_json,
    })
}

fn bind_control_socket() -> Result<UnixListener> {
    let path = Path::new(DEFAULT_SOCKET);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if path.exists() {
        if control::is_running() {
            bail!("another rupa server is already running on {DEFAULT_SOCKET}");
        }
        fs::remove_file(path)
            .with_context(|| format!("failed to remove stale {}", path.display()))?;
    }
    let listener = UnixListener::bind(path)
        .with_context(|| format!("failed to bind the control socket {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to chmod {}", path.display()))?;
    Ok(listener)
}

fn parse_listeners(server: &Server) -> Result<Vec<SocketAddr>> {
    server
        .dns_listen_addrs()
        .iter()
        .map(|value| {
            value
                .parse()
                .with_context(|| format!("invalid DNS listen address '{value}'"))
        })
        .collect()
}

fn setup_link(server: &Server) -> Result<()> {
    let cidr = format!("{}/{}", server.address, server.subnet.prefix_len());
    run_ip(&["addr", "add", &cidr, "dev", &server.interface])?;
    let mtu = server.mtu.to_string();
    run_ip(&["link", "set", "dev", &server.interface, "mtu", &mtu])?;
    run_ip(&["link", "set", "dev", &server.interface, "up"])?;
    Ok(())
}

fn run_ip(args: &[&str]) -> Result<()> {
    let output = Command::new("ip")
        .args(args)
        .output()
        .context("failed to run `ip`")?;
    if !output.status.success() {
        bail!(
            "`ip {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn delete_link(interface: &str) {
    let _ = Command::new("ip")
        .args(["link", "del", "dev", interface])
        .output();
}

fn key_array(bytes: &[u8], what: &str) -> Result<[u8; 32]> {
    bytes
        .try_into()
        .with_context(|| format!("{what} is not 32 bytes"))
}

struct LinkGuard {
    interface: String,
}

impl LinkGuard {
    fn new(interface: &str) -> Self {
        Self {
            interface: interface.to_string(),
        }
    }
}

impl Drop for LinkGuard {
    fn drop(&mut self) {
        delete_link(&self.interface);
    }
}

struct SocketGuard;

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(DEFAULT_SOCKET);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_fields_detects_identity_changes() {
        let private = [1u8; 32];
        let server = Server {
            id: 1,
            name: "test".into(),
            private_key: private.to_vec(),
            public_key: keys::public_from_private(private).to_vec(),
            listen_port: 51820,
            address: "10.8.0.1".parse().unwrap(),
            subnet: "10.8.0.0/24".parse().unwrap(),
            mtu: 1420,
            endpoint: Some("vpn.example.com".into()),
            interface: "rupa0".into(),
            dns_zone: Some("internal.example.com".into()),
            dns_upstreams: "1.1.1.1".into(),
            dns_listen: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        };

        let mut changed = server.clone();
        changed.private_key = [2u8; 32].to_vec();
        changed.listen_port = 51821;
        let fields = restart_fields(&server, &changed);
        assert!(fields.contains(&"private_key"));
        assert!(fields.contains(&"listen_port"));
        assert!(!fields.contains(&"endpoint"));

        let mut endpoint_only = server.clone();
        endpoint_only.endpoint = Some("other.example.com".into());
        assert!(restart_fields(&server, &endpoint_only).is_empty());
    }
}
