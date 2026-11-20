#!/usr/bin/env bash
# build-deb.sh — assemble the binary and its packaging into a .deb.
#
#   packaging/build-deb.sh [--binary PATH] [--out DIR]
#
# Package layout:
#   /usr/bin/rupa                            the binary
#   /usr/lib/systemd/system/rupa.service     the unit (override with a drop-in)
#   /etc/rupa/rupa.env                       conffile: dpkg never overwrites it
#                                            once it has been edited
#   /var/lib/rupa/                           state directory (0700, holds rupa.db)
#   /usr/share/doc/rupa/                     README, CHANGELOG, copyright
#
# /run/rupa is created by systemd (RuntimeDirectory=), nothing to package.
# The database is not shipped: `rupa init` creates it.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
BINARY="$ROOT/target/release/rupa"
OUT="$ROOT/dist"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n1)
ARCH=${ARCH:-$(dpkg --print-architecture)}
MAINTAINER="Alice <337614769+alice-agent-zc@users.noreply.github.com>"

while [ $# -gt 0 ]; do
  case "$1" in
    --binary) BINARY=${2:?--binary needs a path}; shift 2 ;;
    --out) OUT=${2:?--out needs a path}; shift 2 ;;
    -h | --help) sed -n '2,18p' "$0"; exit 0 ;;
    *) echo "build-deb: unknown argument: $1" >&2; exit 2 ;;
  esac
done

[ -x "$BINARY" ] || { echo "build-deb: no binary at $BINARY (cargo build --release first)" >&2; exit 1; }
[ -n "$VERSION" ] || { echo "build-deb: no version in Cargo.toml" >&2; exit 1; }
command -v dpkg-deb >/dev/null || { echo "build-deb: dpkg-deb is missing" >&2; exit 1; }

STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
PKG="$STAGE/rupa"

install -d \
  "$PKG/DEBIAN" \
  "$PKG/usr/bin" \
  "$PKG/usr/lib/systemd/system" \
  "$PKG/usr/share/doc/rupa" \
  "$PKG/etc/rupa" \
  "$PKG/var/lib/rupa"

install -m 0755 "$BINARY" "$PKG/usr/bin/rupa"
install -m 0644 "$ROOT/packaging/rupa.service" "$PKG/usr/lib/systemd/system/rupa.service"
install -m 0644 "$ROOT/packaging/rupa.env" "$PKG/etc/rupa/rupa.env"
install -m 0644 "$ROOT/README.md" "$PKG/usr/share/doc/rupa/README.md"
install -m 0644 "$ROOT/CHANGELOG.md" "$PKG/usr/share/doc/rupa/CHANGELOG.md"
chmod 0700 "$PKG/var/lib/rupa"

cat >"$PKG/usr/share/doc/rupa/copyright" <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: rupa
Source: https://github.com/raonagos/rupa

Files: *
Copyright: 2026 Alice <337614769+alice-agent-zc@users.noreply.github.com>
License: Apache-2.0
 On Debian systems the full text of the Apache-2.0 license can be found in
 /usr/share/common-licenses/Apache-2.0.

Files: vendor/wireguard-rs/*
Copyright: 2020 Mathias Hall-Andersen
License: MIT
EOF
chmod 0644 "$PKG/usr/share/doc/rupa/copyright"

cat >"$PKG/DEBIAN/control" <<EOF
Package: rupa
Version: $VERSION
Architecture: $ARCH
Maintainer: $MAINTAINER
Installed-Size: $(du -sk --exclude=DEBIAN "$PKG" | cut -f1)
Depends: libc6 (>= 2.34), iproute2
Recommends: wireguard-tools
Section: net
Priority: optional
Homepage: https://github.com/raonagos/rupa
Description: WireGuard VPN server and private DNS, configured in SQLite
 rupa creates a TUN device, runs a userspace WireGuard server and serves a
 private DNS zone on the tunnel address. Peers, addresses and DNS records live
 in a SQLite database; client configurations are rendered as wg-quick files.
 .
 It is the private-network companion to uping: the reverse proxy serves the
 private sites, rupa provides the tunnel and the names.
EOF

# Conffiles: dpkg keeps a locally edited copy and prompts on upgrade instead of
# overwriting it. The database under /var/lib/rupa is data, not a conffile.
cat >"$PKG/DEBIAN/conffiles" <<'EOF'
/etc/rupa/rupa.env
EOF

cat >"$PKG/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e

if [ "$1" = configure ]; then
    systemctl daemon-reload >/dev/null 2>&1 || true
    systemctl enable rupa.service >/dev/null 2>&1 || true

    # A fresh install has no database yet: `rupa init` comes first, and starting
    # the daemon before that would only fail. On upgrade the database is there,
    # so restart to run the new binary.
    if [ -f /var/lib/rupa/rupa.db ]; then
        systemctl restart rupa.service >/dev/null 2>&1 || true
    fi
fi

exit 0
EOF

cat >"$PKG/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e

if [ "$1" = remove ]; then
    systemctl stop rupa.service >/dev/null 2>&1 || true
fi

exit 0
EOF

cat >"$PKG/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e

if [ "$1" = remove ] || [ "$1" = purge ]; then
    systemctl daemon-reload >/dev/null 2>&1 || true
    systemctl reset-failed rupa.service >/dev/null 2>&1 || true
fi

exit 0
EOF

chmod 0755 "$PKG/DEBIAN/postinst" "$PKG/DEBIAN/prerm" "$PKG/DEBIAN/postrm"
install -d "$OUT"
DEB="$OUT/rupa_${VERSION}_${ARCH}.deb"
dpkg-deb --root-owner-group --build "$PKG" "$DEB" >/dev/null
echo "$DEB"
