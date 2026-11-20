# rupa

WireGuard VPN server and private DNS, configured in SQLite. Built for a VPS,
CLI first.

> **rupa** — from Tahitian *pāruru*, to protect.

## Status

`v0.2.0` — feature-complete, tested end-to-end with a kernel WireGuard
client (TUN, handshake, private DNS, HTTP through the tunnel, reload,
shutdown).

- [x] `rupa init` — SQLite database, server keypair, endpoint, DNS settings
- [x] `rupa up` / `rupa down` / `rupa reload` — userspace WireGuard runtime
- [x] `rupa peer add|list|show|remove` — client configs, `--output`, `--qr`
- [x] `rupa dns add|list|remove|import` — private DNS records, hosts(5) import
- [x] embedded DNS responder (UDP/TCP, A/PTR/SOA, forwarding)
- [x] `rupa status`, `rupa doctor`, live handshake/transfer counters
- [x] systemd unit

Not yet: web UI, multi-VPS orchestration.

Rupa is the private-network companion to
[uping](https://github.com/raonagos/uping): uping serves the private sites on
`10.8.0.1`, rupa provides the tunnel and the DNS that resolves them.

## Install

Debian/Ubuntu package (recommended):

```sh
curl -LO https://github.com/raonagos/rupa/releases/download/v0.2.0/rupa_0.2.0_amd64.deb
sudo dpkg -i rupa_0.2.0_amd64.deb
```

It installs `/usr/bin/rupa` and the unit
`/usr/lib/systemd/system/rupa.service`, ships `/etc/rupa/rupa.env` as a
conffile (dpkg keeps your version on upgrade, it never overwrites it), and
creates `/var/lib/rupa` (0700) for the database. The service is enabled but only
started once the database exists, so run `rupa init` first.

Or the plain binary, for any glibc x86_64 Linux:

```sh
curl -LO https://github.com/raonagos/rupa/releases/download/v0.2.0/rupa
curl -LO https://github.com/raonagos/rupa/releases/download/v0.2.0/SHA256SUMS
sha256sum -c SHA256SUMS
sudo install -m 0755 rupa /usr/bin/rupa
```

Or build from source: `cargo build --release`, then `packaging/build-deb.sh`
for a package, or install `target/release/rupa` by hand. The release binary is
built on Ubuntu 22.04 and needs a glibc Linux x86_64 system.

## Quick start

```sh
sudo rupa init --endpoint vpn.example.com --dns-zone internal.example.com
sudo systemctl start rupa

# Firewall: the UDP port for clients, and the TUN for DNS/uping.
sudo ufw allow 51820/udp
sudo ufw allow in on rupa0

sudo rupa peer add phone --dns phone --qr
```

The database is `/var/lib/rupa/rupa.db` (override with `--db` or `RUPA_DB`,
including in `/etc/rupa/rupa.env`); `rupa init` prints the chosen address, port,
interface and public key. `rupa up` must run as root (TUN, UDP and DNS :53); the
systemd unit does that.

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

## IPv6 and full tunnel

`rupa init` gives every server a unique-local IPv6 `/64` (random inside
`fd00::/8`, or `--v6-subnet fd..::/64`; `--no-ipv6` opts out). The server takes
`::1`, peers get `::2`, `::3`, …, the TUN carries both families and the private
zone answers `AAAA` with the server address.

`--full-tunnel` routes everything through the server: client configs get
`AllowedIPs = 0.0.0.0/0, ::/0`, and the daemon enables forwarding and installs
`MASQUERADE`/`FORWARD` rules on the default-route interface (removed on
shutdown; a failure is logged and never stops the tunnel from starting).

Both can be changed on an existing database, no re-init:

```sh
sudo rupa config set --v6-subnet fd12:3456:789a::/64
sudo rupa config set --full-tunnel on
sudo systemctl restart rupa          # or: sudo rupa reload (reports what needs it)
```

## Private site behind uping

Goal: a site listening on `127.0.0.1:14080` on the VPS, served as
`site1.internal.rao-nagos.pf`, reachable only through the VPN.

Uping is the reverse proxy; rupa is the tunnel **and the resolver**. The client
must resolve the name before it can connect:

1. the client asks rupa DNS (`10.8.0.1`, set by `DNS =` in the client config)
   for `site1.internal.rao-nagos.pf`;
2. rupa answers `10.8.0.1` from its SQLite records;
3. the client connects to `10.8.0.1:443` through the tunnel;
4. uping, listening only on `10.8.0.1:443`, terminates TLS for that name and
   proxies to `127.0.0.1:14080`.

```toml
# /etc/uping/config.toml
[[server]]
domain       = "site1.internal.rao-nagos.pf"
upstream     = "127.0.0.1:14080"
https_listen = "10.8.0.1"     # private listener: tunnel only
keys         = "internal"     # or "manual"
```

`https_listen` is what makes the site private: uping accepts it only on the
tunnel address, while the upstream stays on localhost. Adding a listener needs
an uping restart (`systemctl restart uping`), not a reload.

Publish the name in rupa; `uping --dns-zone` derives the address from
`https_listen`:

```sh
uping --dns-zone /etc/uping/config.toml | sudo rupa dns import - --prune
sudo rupa dns list
# site1.internal.rao-nagos.pf   10.8.0.1   uping
```

The client config from `rupa peer add` already sets `DNS = 10.8.0.1`, so no
per-device `/etc/hosts` is needed. The A record must be **`10.8.0.1`**, never
`127.0.0.1`: `127.0.0.1` on the client is the client itself. The site never
listens publicly; only uping on the tunnel address does.

With `keys = "internal"`, step-ca must resolve the name to `10.8.0.1` and reach
port 80 on that address for http-01. Uping's HTTP listener is host-wide: if
`[listen] ipv4` is the public address only, either bind it to `0.0.0.0` or use
a manual certificate (`keys = "manual"`, `serve_http = false`) issued by
step-ca. The client must trust the step-ca root in either case.

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
`rupa doctor` checks these rules and prints only the missing ones.

The WireGuard socket is UDP: `ss -tnlp` only lists TCP. Inspect it with:

```sh
sudo ss -unlp | grep 51820
```

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
