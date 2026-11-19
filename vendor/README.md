# vendor/

Third-party sources kept in-tree because they need a local fix and cannot be
shipped as a released crate.

## wireguard-rs

- Upstream: <https://github.com/conduition/wireguard-rs>
- Revision: `dfe5aeffa898ccecefd77e15519bec1b56c33278` (the pin in `Cargo.toml`)
- License: MIT (see `wireguard-rs/LICENSE`)

Copied verbatim from the cargo checkout of that revision, with one change in
`src/wireguard/handshake/device.rs`:

```rust
pub fn release(&self, id: u32) {
    // was: assert!(old.is_some(), "released id not allocated");
    if self.id_map.remove(&id).is_none() {
        log::warn!("release of handshake id {id} which was not allocated (ignored)");
    }
}
```

### Why

`Device::release` asserted that the id was still allocated, but the assertion
fires in production: a two-peer server panicked three to four times a day with
`released id not allocated`.

The id table is an index from local handshake id to peer, used to demultiplex
handshake responses and cookie replies. It is not an ownership record: ids are
handed over between the handshake state and the key-wheel, and several paths
retire or rotate a keypair while the handshake layer still holds the same id.
Removing an id that is already gone is a no-op for an index (the kernel uses
`idr_remove`, wireguard-go ignores the missing-entry error).

The panic itself is what made this fatal. `WireGuard::new` spawns one handshake
worker per CPU and nothing supervises them, so each panic permanently removed
one worker. After `nproc` occurrences the process was still `active`, still
answering `rupa status` and DNS, but could never complete another handshake:
on 4 vCPU, four panics left the tunnel dead for 14 h until a restart.
