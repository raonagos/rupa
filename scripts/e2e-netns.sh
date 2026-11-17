#!/usr/bin/env bash
# End-to-end test for rupa on a disposable Linux host.
#
# Run as root (creates a network namespace, a veth pair, a TUN device and
# binds UDP/51821 + 10.8.0.1:53):
#
#     sudo ./scripts/e2e-netns.sh
#
# The client is the kernel WireGuard implementation inside the namespace.
# Everything is removed on exit.
set -euo pipefail

[ "$(id -u)" -eq 0 ] || {
    echo "run as root: sudo $0" >&2
    exit 1
}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${RUPA_BIN:-$ROOT/target/release/rupa}"
[ -x "$BIN" ] || {
    echo "build the release binary first: cargo build --release" >&2
    exit 1
}

for tool in ip wg wg-quick dig curl ping python3; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "missing tool: $tool" >&2
        exit 1
    }
done

NS="rupa-e2e-client"
SERVER_IF="rupa-e2e0"
VETH_HOST="rupa-e2e-host"
VETH_CLIENT="rupa-e2e-cli"
PORT="${PORT:-51821}"
HTTP_PORT="${HTTP_PORT:-18080}"
TMP="$(mktemp -d)"
SERVER_PID=""
HTTP_PID=""
FIREWALL_CHAIN="RUPA-E2E"

remove_firewall_rules() {
    iptables -D INPUT -j "$FIREWALL_CHAIN" >/dev/null 2>&1 || true
    iptables -F "$FIREWALL_CHAIN" >/dev/null 2>&1 || true
    iptables -X "$FIREWALL_CHAIN" >/dev/null 2>&1 || true
}

cleanup() {
    set +e
    if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
        "$BIN" --db "$TMP/rupa.db" down >/dev/null 2>&1
        sleep 1
        kill "$SERVER_PID" 2>/dev/null
    fi
    if [ -n "$HTTP_PID" ]; then
        kill "$HTTP_PID" 2>/dev/null
        wait "$HTTP_PID" 2>/dev/null || true
    fi
    remove_firewall_rules
    ip netns exec "$NS" wg-quick down "$TMP/client.conf" >/dev/null 2>&1
    ip netns del "$NS" >/dev/null 2>&1
    ip link del "$VETH_HOST" >/dev/null 2>&1
    rm -rf "$TMP"
}
trap cleanup EXIT

# Kernel WireGuard (built-in or module).
modprobe wireguard >/dev/null 2>&1 || true

ip netns del "$NS" >/dev/null 2>&1 || true
ip link del "$VETH_HOST" >/dev/null 2>&1 || true

echo "==> network namespace and veth pair"
ip netns add "$NS"
ip link add "$VETH_HOST" type veth peer name "$VETH_CLIENT"
ip link set "$VETH_CLIENT" netns "$NS"
ip addr add 192.168.99.1/24 dev "$VETH_HOST"
ip link set "$VETH_HOST" up
ip netns exec "$NS" ip addr add 192.168.99.2/24 dev "$VETH_CLIENT"
ip netns exec "$NS" ip link set "$VETH_CLIENT" up
ip netns exec "$NS" ip link set lo up

echo "==> database and peer"
"$BIN" --db "$TMP/rupa.db" init \
    --endpoint 192.168.99.1 \
    --port "$PORT" \
    --interface "$SERVER_IF" \
    --subnet 10.8.0.0/24 \
    --dns-zone internal.example.com >/dev/null
"$BIN" --db "$TMP/rupa.db" peer add client --dns client --output "$TMP/client.conf" >/dev/null
"$BIN" --db "$TMP/rupa.db" dns add site1.internal.example.com 10.8.0.1 >/dev/null

# wg-quick inside the namespace has no resolvconf; the resolver is tested with dig.
sed -i '/^DNS =/d' "$TMP/client.conf"

echo "==> starting rupa"
RUST_LOG="${RUST_LOG:-wireguard=debug,rupa=debug}" \
    "$BIN" --db "$TMP/rupa.db" up >"$TMP/server.log" 2>&1 &
SERVER_PID=$!

for _ in $(seq 1 100); do
    if [ -S /run/rupa/rupa.sock ]; then
        break
    fi
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        echo "rupa exited during startup:" >&2
        cat "$TMP/server.log" >&2
        exit 1
    fi
    sleep 0.1
done
[ -S /run/rupa/rupa.sock ] || {
    echo "control socket did not appear:" >&2
    cat "$TMP/server.log" >&2
    exit 1
}

echo "==> sockets"
ss -lunp | grep -E ":($PORT|53)\b" || true

echo "==> veth sanity check"
ip netns exec "$NS" ping -c1 -W1 192.168.99.1

# ufw's INPUT policy is DROP on many hosts, including this one. Allow the
# WireGuard port and the tunnel interface for the duration of the test; real
# deployments need the same two rules.
if command -v iptables >/dev/null 2>&1; then
    remove_firewall_rules
    iptables -N "$FIREWALL_CHAIN"
    iptables -A "$FIREWALL_CHAIN" -p udp --dport "$PORT" -j ACCEPT
    iptables -A "$FIREWALL_CHAIN" -i "$SERVER_IF" -j ACCEPT
    iptables -I INPUT 1 -j "$FIREWALL_CHAIN"
else
    echo "warning: iptables not found; the host firewall may block the test" >&2
fi

echo "==> HTTP server on 10.8.0.1:$HTTP_PORT"
(
    cd "$TMP"
    exec python3 -m http.server "$HTTP_PORT" --bind 10.8.0.1
) >"$TMP/http.log" 2>&1 &
HTTP_PID=$!

for _ in $(seq 1 50); do
    if curl -fsS -o /dev/null "http://10.8.0.1:$HTTP_PORT/" 2>/dev/null; then
        break
    fi
    if ! kill -0 "$HTTP_PID" 2>/dev/null; then
        echo "HTTP test server exited:" >&2
        cat "$TMP/http.log" >&2
        exit 1
    fi
    sleep 0.1
done
curl -fsS -o /dev/null "http://10.8.0.1:$HTTP_PORT/" || {
    echo "HTTP test server is not reachable:" >&2
    cat "$TMP/http.log" >&2
    exit 1
}

echo "==> starting the kernel WireGuard client"
ip netns exec "$NS" wg-quick up "$TMP/client.conf"

handshake=0
for _ in $(seq 1 50); do
    if ip netns exec "$NS" ping -c1 -W1 10.8.0.1 >/dev/null 2>&1; then
        handshake=1
        break
    fi
    sleep 0.2
done
if [ "$handshake" -ne 1 ]; then
    echo "FAIL: no handshake" >&2
    ip netns exec "$NS" wg show >&2 || true
    echo "--- server log" >&2
    cat "$TMP/server.log" >&2
    echo "--- sockets" >&2
    ss -lunp >&2 || true
    echo "--- routes" >&2
    ip route >&2
    ip netns exec "$NS" ip route >&2
    echo "--- firewall" >&2
    nft list ruleset 2>/dev/null | head -60 >&2 || true
    iptables -S 2>/dev/null | head -60 >&2 || true
    ufw status 2>/dev/null >&2 || true
    exit 1
fi

echo "==> checks"
ip netns exec "$NS" ping -c2 -W2 10.8.0.1

answer="$(ip netns exec "$NS" dig @10.8.0.1 +time=2 +tries=1 +short site1.internal.example.com A)"
[ "$answer" = "10.8.0.1" ] || {
    echo "FAIL: DNS A answer was '$answer'" >&2
    exit 1
}

ptr="$(ip netns exec "$NS" dig @10.8.0.1 +time=2 +tries=1 +short -x 10.8.0.2)"
case "$ptr" in
    client.internal.example.com.*) ;;
    *)
        echo "FAIL: PTR answer was '$ptr'" >&2
        exit 1
        ;;
esac

code="$(ip netns exec "$NS" curl -sS -o /dev/null -w '%{http_code}' --max-time 5 "http://10.8.0.1:$HTTP_PORT/")"
[ "$code" = "200" ] || {
    echo "FAIL: HTTP through the tunnel returned $code" >&2
    exit 1
}

"$BIN" --db "$TMP/rupa.db" status --json | grep -q '"runtime": true'
"$BIN" --db "$TMP/rupa.db" peer list --live | grep -q "client"
"$BIN" --db "$TMP/rupa.db" reload | grep -q "Reloaded"

echo "==> stopping rupa"
remove_firewall_rules
"$BIN" --db "$TMP/rupa.db" down
for _ in $(seq 1 50); do
    kill -0 "$SERVER_PID" 2>/dev/null || break
    sleep 0.1
done
if kill -0 "$SERVER_PID" 2>/dev/null; then
    echo 'FAIL: rupa did not stop after rupa down' >&2
    exit 1
fi
[ ! -e /run/rupa/rupa.sock ] || {
    echo "FAIL: control socket was not removed" >&2
    exit 1
}

echo "PASS: rupa v0.1.0 end-to-end"
