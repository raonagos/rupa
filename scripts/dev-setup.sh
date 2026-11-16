#!/usr/bin/env bash
# =============================================================================
# rupa dev setup — Docker Engine, passwordless sudo, and a TUN smoke test.
#
# Run with sudo (the script refuses to run otherwise):
#
#     sudo ./scripts/dev-setup.sh
#     sudo ./scripts/dev-setup.sh --user tetuaoro
#
# It is idempotent: re-running it only fixes what is missing.
#
# What it does:
#   1. installs Docker Engine + buildx/compose from Docker's apt repository
#   2. adds the target user to the docker group
#   3. writes /etc/sudoers.d/90-rupa-dev with NOPASSWD:ALL
#   4. starts docker and verifies it
#   5. creates a TUN device inside a privileged container — the capability
#      rupa needs, and the reason this script exists
#
# Options:
#   --user NAME        target user (default: $SUDO_USER)
#   --no-sudoers       leave /etc/sudoers.d alone
#   --no-docker-test   skip the alpine/TUN container test
#   -h, --help
#
# Revoke:
#   sudo rm -f /etc/sudoers.d/90-rupa-dev
#   sudo gpasswd -d tetuaoro docker
#
# Security: NOPASSWD:ALL is a big hammer. This is a development machine, not
# a production VPS. Read the warning printed at the end.
#
# Note: a shell that is already sandboxed with no_new_privs cannot use sudo
# even with NOPASSWD. Docker is the useful path in that case: the daemon runs
# privileged, and the client only needs access to /var/run/docker.sock.
# =============================================================================
set -euo pipefail

log()  { printf '\033[1;32m[+]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[!]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[x]\033[0m %s\n' "$*" >&2; exit 1; }

TARGET_USER=""
WITH_SUDOERS=1
DOCKER_TEST=1

while [ $# -gt 0 ]; do
    case "$1" in
        --user)
            [ $# -ge 2 ] || die "--user needs a value"
            TARGET_USER="$2"
            shift 2
            ;;
        --no-sudoers)
            WITH_SUDOERS=0
            shift
            ;;
        --no-docker-test)
            DOCKER_TEST=0
            shift
            ;;
        -h|--help)
            sed -n '2,40p' "$0"
            exit 0
            ;;
        *)
            die "unknown argument: $1 (try --help)"
            ;;
    esac
done

# ---------------------------------------------------------------------------
# 0. sanity
# ---------------------------------------------------------------------------
[ "$(id -u)" -eq 0 ] || die "run this with sudo (it needs root)"

if [ -z "$TARGET_USER" ]; then
    TARGET_USER="${SUDO_USER:-}"
fi
[ -n "$TARGET_USER" ] || die "cannot detect the target user; pass --user NAME"
[ "$TARGET_USER" != "root" ] || die "refusing to target root; pass --user NAME"
id "$TARGET_USER" >/dev/null 2>&1 || die "no such user: $TARGET_USER"

[ -r /etc/os-release ] || die "no /etc/os-release"
# shellcheck disable=SC1091
. /etc/os-release
case "${ID:-}" in
    ubuntu|debian) ;;
    *) die "unsupported distro: ${ID:-unknown} (ubuntu/debian only)" ;;
esac
CODENAME="${VERSION_CODENAME:-}"
[ -n "$CODENAME" ] || die "no VERSION_CODENAME in /etc/os-release"
ARCH="$(dpkg --print-architecture)"

log "target user: $TARGET_USER ($(id -un "$TARGET_USER") / $(id -gn "$TARGET_USER"))"
log "distro: ${PRETTY_NAME:-$ID $CODENAME} / $ARCH"

# ---------------------------------------------------------------------------
# 1. base tools (TUN config, DNS checks, kernel WireGuard client for tests)
# ---------------------------------------------------------------------------
log "installing base packages"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y --no-install-recommends \
    ca-certificates curl gnupg iproute2 nftables wireguard-tools sudo

# ---------------------------------------------------------------------------
# 2. Docker Engine from Docker's official apt repository
# ---------------------------------------------------------------------------
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    log "docker is already installed and running"
else
    log "adding Docker's apt repository"
    install -m 0755 -d /etc/apt/keyrings
    curl -fsSL "https://download.docker.com/linux/${ID}/gpg" \
        -o /etc/apt/keyrings/docker.asc
    chmod a+r /etc/apt/keyrings/docker.asc

    cat >/etc/apt/sources.list.d/docker.list <<EOF
deb [arch=${ARCH} signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/${ID} ${CODENAME} stable
EOF

    apt-get update -qq
    log "installing docker engine"
    apt-get install -y --no-install-recommends \
        docker-ce docker-ce-cli containerd.io \
        docker-buildx-plugin docker-compose-plugin
fi

if [ -d /run/systemd/system ]; then
    systemctl enable --now docker >/dev/null 2>&1 \
        || warn "could not enable/start docker via systemd"
else
    warn "systemd is not running; start dockerd manually if needed"
fi

for _ in 1 2 3 4 5; do
    docker info >/dev/null 2>&1 && break
    sleep 1
done
docker info >/dev/null 2>&1 || die "the docker daemon is not responding"

# ---------------------------------------------------------------------------
# 3. docker group
# ---------------------------------------------------------------------------
groupadd -f docker
if id -nG "$TARGET_USER" | tr ' ' '\n' | grep -qx docker; then
    log "$TARGET_USER is already in the docker group"
else
    usermod -aG docker "$TARGET_USER"
    log "added $TARGET_USER to the docker group"
fi

# ---------------------------------------------------------------------------
# 4. passwordless sudo
# ---------------------------------------------------------------------------
SUDOERS_FILE=/etc/sudoers.d/90-rupa-dev
if [ "$WITH_SUDOERS" -eq 1 ]; then
    log "configuring passwordless sudo for $TARGET_USER"
    tmp="$(mktemp)"
    printf '%s ALL=(ALL) NOPASSWD:ALL\n' "$TARGET_USER" >"$tmp"
    chmod 0440 "$tmp"
    if ! visudo -cf "$tmp" >/dev/null; then
        rm -f "$tmp"
        die "refusing to install an invalid sudoers entry"
    fi
    install -m 0440 "$tmp" "$SUDOERS_FILE"
    rm -f "$tmp"
    log "wrote $SUDOERS_FILE"
else
    warn "skipping sudoers (--no-sudoers)"
fi

# ---------------------------------------------------------------------------
# 5. verify
# ---------------------------------------------------------------------------
if [ "$WITH_SUDOERS" -eq 1 ]; then
    if runuser -u "$TARGET_USER" -- sudo -n true 2>/dev/null; then
        log "passwordless sudo works for $TARGET_USER"
    else
        warn "sudo -n failed for $TARGET_USER (sandbox no_new_privs?)"
    fi
fi

if runuser -u "$TARGET_USER" -g docker -- docker info >/dev/null 2>&1; then
    log "docker socket is accessible to $TARGET_USER (docker group)"
else
    warn "docker socket not accessible yet; log out/in, or use: sg docker -c 'docker ...'"
fi

if [ "$DOCKER_TEST" -eq 1 ]; then
    log "testing TUN creation in a privileged container"
    if runuser -u "$TARGET_USER" -g docker -- docker run --rm \
        --cap-add=NET_ADMIN --device /dev/net/tun \
        alpine:3.20 sh -c \
        'apk add --no-cache iproute2 >/dev/null && ip tuntap add dev rupa-test mode tun && ip link del rupa-test'; then
        log "TUN inside a container: OK"
    else
        warn "TUN container test failed (offline? alpine pull?) — not fatal"
    fi
fi

# ---------------------------------------------------------------------------
# summary
# ---------------------------------------------------------------------------
cat <<EOF

--------------------------------------------------------------------------
rupa dev setup — done
  user:   $TARGET_USER
  docker: $(docker --version 2>/dev/null || echo 'not installed')
  sudo:   $([ -f "$SUDOERS_FILE" ] && echo "NOPASSWD:ALL -> $SUDOERS_FILE" || echo 'unchanged')

Next:
  newgrp docker                 # or log out / log in
  docker run --rm hello-world
  sudo -n true && echo 'sudo without password: OK'

  # in a shell that cannot newgrp:
  sg docker -c 'docker info'

Revoke:
  sudo rm -f $SUDOERS_FILE
  sudo gpasswd -d $TARGET_USER docker

WARNING: NOPASSWD:ALL means any process running as $TARGET_USER can become
root without a password. Keep this on a development machine only.

NOTE: if your agent shell runs with no_new_privs, sudo will still fail there
even with NOPASSWD. Use Docker from that shell (the daemon is privileged and
the client only needs the socket).
--------------------------------------------------------------------------
EOF
