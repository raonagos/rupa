# rupa — plan v0.1.0

Status: validated for implementation (2026-11-16).

## Verdict

Feasible. The WireGuard data path is
[conduition/wireguard-rs](https://github.com/conduition/wireguard-rs), pinned at
`dfe5aeffa898ccecefd77e15519bec1b56c33278` (crate `wireguard` 0.2.0). It builds
with Rust 1.98 and exposes the API a dynamic server needs: `add_peer`,
`remove_peer`, `add_allowed_ip`, `set_private_key`, `set_listen_port`, `up`,
`add_udp_reader`, `add_tun_reader`, `get_peers`. Client endpoints are learned
from the handshake (NAT roaming), and inbound source addresses are checked
against the peer's allowed IPs.

Two caveats are accepted for 0.1.0:

1. The library is a patched fork of the reference implementation, last touched
   in September 2025. It pulls 2020-era crypto crates; `curve25519-dalek 3.0.0`
   is flagged by [RUSTSEC-2024-0344](https://rustsec.org/advisories/RUSTSEC-2024-0344.html).
   The advisory concerns `Scalar::sub`, not the X25519 Montgomery ladder used by
   WireGuard, but it is a flagged dependency. Mitigation: pin the revision, run
   `cargo audit` and document the result, keep the engine behind a small
   boundary so it can be replaced by `boringtun` or kernel WireGuard.
2. The library's own test suite does not build on modern Rust (a 2020
   `rustc-serialize` dev-dependency). The library itself builds; rupa's tests
   cover the integration.

## Decisions

- **License:** Apache-2.0, aligned with `uping`.
- **Scope 0.1.0:** private network (split tunnel). Clients route
  `10.8.0.0/24` and use rupa's DNS. NAT/full tunnel is 0.2.0.
- **One server per database**, but the schema is multi-server from day one.
- **No configuration file.** SQLite is the only source of truth.
- **Repository:** `raonagos/rupa`; commits authored as Alice
  (`337614769+alice-agent-zc@users.noreply.github.com`). The local `gh` token is
  `tetuaoro` (admin of `raonagos`), so that account creates the repo and pushes.
- **Dev/test environment:** Docker is installed on the development machine so
  the privileged TUN path can run in a container. See
  [`scripts/dev-setup.sh`](../scripts/dev-setup.sh).
- **On the harness VPS:** do not collide with the existing `wg0`:
  use port `51821/udp` and subnet `10.8.0.0/24` (the subnet used by uping's
  private-site examples), interface `rupa0`.

## Architecture

```
client wg-quick ── UDP/51820 ──> rupa (userspace wireguard-rs)
                                    ├── TUN rupa0  10.8.0.1/24
                                    ├── SQLite /var/lib/rupa/rupa.db
                                    │     keys, peers, DNS records, settings
                                    ├── DNS 10.8.0.1:53 (+ 127.0.0.1 optional)
                                    └── /run/rupa/rupa.sock
                                          reload · stats · shutdown
                                             ▲
                                          rupa CLI
```

- The CLI writes the database in a transaction, then asks the running server to
  reload over the Unix socket. Without a running server the change applies at
  the next `up`.
- Reload adds/removes peers, refreshes allowed IPs/endpoints, and swaps the DNS
  table. Changing the private key, listen port or interface name needs a
  restart.
- The server binds the tunnel address before starting DNS, so DNS can listen on
  `10.8.0.1:53`.

## CLI (0.1.0)

```text
rupa init --endpoint vpn.example.com [--subnet 10.8.0.0/24]
          [--port 51820] [--interface rupa0] [--dns-zone internal.example.com]

rupa up | down | status [--json] | reload | doctor

rupa peer add NAME [--dns NAME] [--output FILE] [--qr]
rupa peer list [--live] [--json]
rupa peer show NAME
rupa peer remove NAME [--yes]

rupa dns add NAME IP
rupa dns list [--json]
rupa dns remove NAME [--yes]
rupa dns import PATH|- [--source uping] [--prune]
```

`peer add` generates the key pair, allocates the first free address, stores
everything, and prints a `wg-quick` client config:

```ini
[Interface]
PrivateKey = ...
Address = 10.8.0.2/24
DNS = 10.8.0.1
MTU = 1420

[Peer]
PublicKey = ...
AllowedIPs = 10.8.0.0/24
Endpoint = vpn.example.com:51820
PersistentKeepalive = 25
```

`--output` writes mode 0600; `--qr` renders the config as a QR code.

## SQLite schema (v1)

```sql
servers(id, name, private_key, public_key, listen_port, address, subnet,
        mtu, endpoint, interface, dns_zone, dns_upstreams, dns_listen,
        created_at)

peers(id, server_id, name, private_key, public_key, address,
      created_at, enabled,
      UNIQUE(server_id, name), UNIQUE(server_id, public_key),
      UNIQUE(server_id, address))

dns_records(id, server_id, name, address, source, peer_id, updated_at,
            UNIQUE(server_id, name))
```

`source` is `manual`, `uping` or `peer`. Removing a peer cascades to its DNS
record. Keys are 32-byte BLOBs; the database is 0600 and its directory 0700.
A backup is a copy of the database.

## DNS

- Embedded UDP + TCP server (`hickory-proto` + tokio), default listener
  `10.8.0.1:53`; `127.0.0.1:53` can be added for local step-ca resolution.
- The configured zone (for example `internal.rao-nagos.pf`) is authoritative
  from SQLite: A records, automatic PTR for peer addresses, and a minimal SOA
  for negative answers. Everything else is forwarded to configurable upstreams.
- uping integration without coupling:
  `uping --dns-zone /etc/uping/config.toml | rupa dns import - --source uping --prune`.
- Rupa does not manage certificates. Clients still install the step-ca root;
  uping still owns ACME/http-01. Rupa only makes the private names resolve.

## Runtime

1. Open the database, check root/`/dev/net/tun`, ensure `rupa0` is absent.
2. Create the TUN, add the address, set MTU, bring the link up.
3. Create the WireGuard device, set the private key, add peers and their `/32`
   allowed IPs, then call `up(mtu)` exactly once. Do not use the library's TUN
   status thread: it can call `up` a second time and rebind the UDP socket.
4. Start DNS, then the control socket and signal handlers.
5. On SIGTERM: remove NAT rules if any, exit; the TUN disappears with the
   process.

`rupa doctor` checks the database, schema, endpoint, `/dev/net/tun`,
`CAP_NET_ADMIN`, `ip`, `nft`, the interface name and the control socket.

## Non-goals for 0.1.0

Web UI/API, multi-VPS orchestration (schema is ready), IPv6, PSK, NAT/full
tunnel, mesh, kernel-WireGuard mode, Windows/macOS server, automatic firewall
changes.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| Library is a patched, rarely maintained fork | pin revision, audit, isolate engine, fallback to `boringtun`/kernel |
| Old crypto dependencies | `cargo audit`, document RUSTSEC-2024-0344, revisit in 0.2.0 |
| Privileged path cannot run in the agent sandbox | Docker dev setup, privileged container e2e, `doctor` |
| Port 51820 already used on the harness VPS | use 51821/udp there; port is configurable |
| Port 53 conflict (dnsmasq/systemd-resolved) | configurable listeners, fail-fast message, `doctor` |
| Database holds private keys | 0600/0700, never log keys, backup documented |
| DNS/ACME trust confusion | rupa only resolves; step-ca root is a client-side install |

## Phases

| Phase | Content | Tests |
|---|---|---|
| P0 | repo, scaffold, CI, license, dev setup script | CI |
| P1 | DB/migrations, keys, `init`, peers, config rendering | unit + `assert_cmd` |
| P2 | DNS UDP/TCP, zone, PTR/SOA, forward, import, reload | tokio + `dig` on 127.0.0.1:5353 |
| P3 | TUN/UDP runtime, control socket, signals, `up/status/reload/down`, `doctor`, systemd | build + reload-diff tests |
| P4 | netns/container e2e, VPS smoke test, docs, `v0.1.0` tag and binary | ping/dig/curl through the tunnel |

Definition of done: a `wg-quick` client connects, pings `10.8.0.1`, resolves a
private name through rupa, and reaches an uping site on `10.8.0.1:443`.
