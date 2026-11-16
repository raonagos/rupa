# rupa

WireGuard VPN server and private DNS, configured in SQLite. Built for a VPS,
CLI first.

> **rupa** — from Tahitian *pāruru*, to protect.

## Status

`v0.1.0` is in development. The plan, feasibility notes and open decisions live
in [docs/PLAN.md](docs/PLAN.md).

Implemented so far:

- [x] `rupa init` — SQLite database, server keypair, endpoint, DNS settings
- [x] `rupa peer add|list|show|remove` — client configs, `--output`, `--qr`
- [x] `rupa dns add|list|remove|import` — private DNS records, hosts(5) import
- [x] embedded DNS responder (UDP/TCP, A/PTR/SOA, forwarding)
- [x] `rupa status`, `rupa doctor`
- [ ] `rupa up` / `rupa reload` — userspace WireGuard runtime (P3)
- [ ] systemd unit, release binary

Not in 0.1.0: web UI, multi-VPS orchestration, IPv6, NAT/full tunnel.

P1 and P2 are in: SQLite config, keys, peers, client configs, DNS records, a
working resolver, and `uping --dns-zone` import. The WireGuard runtime is next.

Rupa is the private-network companion to
[uping](https://github.com/raonagos/uping): uping serves the private sites on
`10.8.0.1`, rupa provides the tunnel and the DNS that resolves them.

## Design in one paragraph

The configuration lives in SQLite, not in a config file. The server reads the
database at startup and reloads peers and DNS records on demand through a local
Unix socket. Client keys are generated on the server and printed as a standard
`wg-quick` configuration. The WireGuard data path is
[conduition/wireguard-rs](https://github.com/conduition/wireguard-rs); the DNS
server is embedded (`hickory-proto`).

## Development

```sh
sudo ./scripts/dev-setup.sh   # Docker + passwordless sudo, dev machines only
cargo build --release
cargo test
```

The resolver can be exercised without the WireGuard runtime:

```sh
cargo run --example dns-serve -- /tmp/rupa-test/rupa.db 127.0.0.1:15353
dig @127.0.0.1 -p 15353 site1.internal.example.com A
```

The end-to-end test needs `CAP_NET_ADMIN` (TUN) and runs in a privileged
container; see [docs/PLAN.md](docs/PLAN.md).

## License

Apache-2.0 — see [LICENSE](LICENSE).
