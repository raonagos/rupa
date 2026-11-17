# rupa

WireGuard VPN server and private DNS, configured in SQLite. Built for a VPS,
CLI first.

> **rupa** — from Tahitian *pāruru*, to protect.

## Status

`v0.1.0` — feature-complete and tested end-to-end with a kernel WireGuard
client (TUN, handshake, private DNS, HTTP through the tunnel, reload,
shutdown).

- [x] `rupa init` — SQLite database, server keypair, endpoint, DNS settings
- [x] `rupa up` / `rupa down` / `rupa reload` — userspace WireGuard runtime
- [x] `rupa peer add|list|show|remove` — client configs, `--output`, `--qr`
- [x] `rupa dns add|list|remove|import` — private DNS records, hosts(5) import
- [x] embedded DNS responder (UDP/TCP, A/PTR/SOA, forwarding)
- [x] `rupa status`, `rupa doctor`, live handshake/transfer counters
- [x] systemd unit

Not in 0.1.0: web UI, multi-VPS orchestration, IPv6, NAT/full tunnel.

Rupa is the private-network companion to
[uping](https://github.com/raonagos/uping): uping serves the private sites on
`10.8.0.1`, rupa provides the tunnel and the DNS that resolves them.

## Install

Prebuilt glibc x86_64 binary for v0.1.0:

```sh
curl -LO https://github.com/raonagos/rupa/releases/download/v0.1.0/rupa
curl -LO https://github.com/raonagos/rupa/releases/download/v0.1.0/rupa.sha256
sha256sum -c rupa.sha256
sudo install -m 0755 rupa /usr/local/bin/rupa
```

Or build from source (`cargo build --release`) and install
`target/release/rupa`. The release binary is built on Ubuntu 22.04 and needs a
glibc Linux x86_64 system.

## Quick start

```sh
sudo rupa init --endpoint vpn.example.com --dns-zone internal.example.com

sudo install -m 0644 packaging/rupa.service /etc/systemd/system/rupa.service
sudo systemctl daemon-reload
sudo systemctl enable --now rupa

# Firewall: the UDP port for clients, and the TUN for DNS/uping.
sudo ufw allow 51820/udp
sudo ufw allow in on rupa0

sudo rupa peer add phone --dns phone --qr
```

The database is `/var/lib/rupa/rupa.db`; `rupa init` prints the chosen address,
port, interface and public key. `rupa up` must run as root (TUN, UDP and DNS
:53); the systemd unit does that.

On the client:

```sh
sudo wg-quick up phone.conf
ping 10.8.0.1
dig @10.8.0.1 site1.internal.example.com
```

Import the private names uping already knows:

```sh
uping --dns-zone /etc/uping/config.toml | sudo rupa dns import - --prune
sudo rupa dns list
```

Rupa only resolves names. A private ACME CA (step-ca) still needs its root
installed on the clients; uping still owns the certificates.

## Firewall

ufw's default incoming policy blocks two different things:

- the WireGuard UDP port, so clients never handshake;
- packets arriving on the TUN interface, so clients cannot reach the DNS
  resolver or uping on `10.8.0.1`.

```sh
sudo ufw allow 51820/udp      # or the --port you chose
sudo ufw allow in on rupa0    # or the --interface you chose
```

With nftables, the equivalents are an `udp dport 51820 accept` input rule and
an `iifname "rupa0" accept` input rule. Rupa does not modify the firewall;
`rupa doctor` warns when ufw is active.

## Design

The configuration lives in SQLite, not in a config file. The server reads the
database at startup and reloads peers and DNS records on demand through a local
Unix socket (`/run/rupa/rupa.sock`). Client keys are generated on the server
and printed as a standard `wg-quick` configuration. The WireGuard data path is
[conduition/wireguard-rs](https://github.com/conduition/wireguard-rs); the DNS
server is embedded (`hickory-proto`).

`rupa up` creates the TUN, assigns the address, starts the userspace WireGuard
device, starts DNS on the tunnel address, and serves `ping`, `stats`, `reload`
and `shutdown` on the control socket. `SIGHUP` and `rupa reload` re-read the
database; changing the private key, listen port, interface, address, subnet,
MTU or DNS listeners needs a restart and is reported by `reload`.

## Development

```sh
sudo ./scripts/dev-setup.sh    # Docker + passwordless sudo, dev machines only
cargo build --release
cargo test                     # 39 tests
```

The end-to-end test needs root and the kernel WireGuard module. It creates a
throwaway network namespace, a veth pair, a TUN device and temporary firewall
rules, runs a kernel WireGuard client, then removes everything:

```sh
sudo ./scripts/e2e-netns.sh
```

The resolver can also be exercised without the WireGuard runtime:

```sh
cargo run --example dns-serve -- /tmp/rupa-test/rupa.db 127.0.0.1:15353
dig @127.0.0.1 -p 15353 site1.internal.example.com A
```

## License

Apache-2.0 — see [LICENSE](LICENSE).
