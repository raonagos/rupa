# Changelog

## [Unreleased]

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
