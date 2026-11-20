# Changelog

## [0.1.3] - 2026-11-19

### Fixed

- `wireguard-rs` is vendored (`vendor/wireguard-rs`) with one local fix:
  `Device::release` no longer panics when a handshake id is released twice.
  The assertion fires in production — three to four times a day on a two-peer
  server — and the panic lands in a handshake worker, which nothing supervises:
  each occurrence permanently removed one worker, and with one worker per CPU
  the tunnel stopped handshaking for good after `nproc` panics while the
  process stayed up and `rupa status` kept answering.
  The id table is an index (local id -> peer) used to demultiplex handshake
  responses and cookie replies, not an ownership record; removing an absent
  entry is a no-op, as in the kernel and wireguard-go.
  The 0.1.2 panic hook stays as a safety net for anything else.

## [0.1.2] - 2026-11-19

### Fixed

- A panic in a worker thread no longer leaves a silently dead tunnel. The
  userspace WireGuard runtime spawns one handshake worker per CPU; when one
  panicked (wireguard-rs `released id not allocated`, on a retransmitted
  handshake initiation), the process stayed up and `rupa status` kept
  answering, but no handshake could complete any more until a manual restart.
  Any panic now aborts the process so systemd's `Restart=on-failure` restarts
  it, and the panic is logged.
- `rupa up` now logs one line at startup (interface, address, mtu, UDP port,
  peer count, DNS listeners, endpoint). Previously a healthy start logged
  nothing at all, which made the journal look empty.

## [0.1.1] - 2026-11-16

### Fixed

- `rupa doctor` no longer reports the UDP port and the TUN interface as
  problems when the server is running: both are held by rupa itself.
- `rupa doctor` now checks the actual ufw rules instead of warning whenever
  ufw is active. It confirms `<port>/udp` and `on <interface>`, and prints only
  the missing rules.

## [0.1.0] - 2026-11-16

### Added

- Initial project scaffold: repository, CI, license, plan, development setup.
- SQLite schema v1 (`servers`, `peers`, `dns_records`) and migrations.
- `rupa init`: server keypair, subnet/IP validation, endpoint detection,
  DNS zone and upstream settings.
- `rupa peer add|list|show|remove`: key generation, address allocation,
  `wg-quick` rendering, secure `--output` and terminal QR codes.
- `rupa dns add|list|remove`: manual and peer-managed records.
- `rupa status` and `rupa doctor`.
- Embedded DNS resolver: UDP and TCP, authoritative private zone, A/PTR/SOA,
  NXDOMAIN/NODATA, and forwarding to upstream resolvers.
- `rupa dns import`: `hosts(5)` input (stdin or file), `--source uping|manual`,
  `--prune`, and protection against overwriting peer-managed names.
- `dns-serve` example for debugging the resolver on a high port.
- Userspace WireGuard runtime on a TUN device (`rupa up`): endpoint learning,
  per-peer allowed IPs, live handshake/transfer counters.
- Control socket with `ping`, `stats`, `reload` and `shutdown`; `rupa down`,
  `rupa reload`, live `rupa status` and `rupa peer list --live`.
- `SIGHUP` reload of peers and DNS records; restart-required fields reported.
- Systemd unit, ufw guidance, end-to-end netns test (ping, DNS, HTTP, reload,
  shutdown).
