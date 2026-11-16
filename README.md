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
- [x] `rupa dns add|list|remove` — private DNS records
- [x] `rupa status`, `rupa doctor`
- [ ] `rupa up` / `rupa reload` — userspace WireGuard runtime (P3)
- [ ] embedded DNS responder (P2)
- [ ] `rupa dns import` — uping `--dns-zone` import (P2)
- [ ] systemd unit, release binary

Not in 0.1.0: web UI, multi-VPS orchestration, IPv6, NAT/full tunnel.

P1 is in: the SQLite schema, keys, peer config rendering and DNS record CRUD.
The server runtime and the embedded DNS responder are next.

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

The end-to-end test needs `CAP_NET_ADMIN` (TUN) and runs in a privileged
container; see [docs/PLAN.md](docs/PLAN.md).

## License

Apache-2.0 — see [LICENSE](LICENSE).
