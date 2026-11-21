#!/usr/bin/env bash
# Builds the image once (it needs the network for apt), then runs each check
# in a fresh container with no network and removes it. Never run check.sh or
# login-path.sh on a machine you care about: the first restarts a unit named
# gdm.service, the second starts a display manager.
#
#   packaging/boot-watch/run.sh path/to/better-manager-daemon \
#       [path/to/better-os-login-supervisor path/to/better-os-login-status]
#
# With the login supervisor's two programs it also runs login-path.sh, the
# Better OS login path's check (ticket 267).
set -euo pipefail
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
BINARY="${1:?the better-manager-daemon binary to test}"
SUPERVISOR="${2:-}"
STATUS="${3:-}"
if [[ -n "$SUPERVISOR" && -z "$STATUS" ]]; then
    echo "run.sh: the login supervisor's check needs better-os-login-status too" >&2
    exit 2
fi
IMAGE=better-os-boot-watch:24.04
NAME="better-os-boot-watch-$$"

docker image inspect "$IMAGE" >/dev/null 2>&1 \
    || docker build -t "$IMAGE" "$ROOT_DIR/packaging/boot-watch"

cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT

# A fresh container with systemd running in it.
start_container() {
    cleanup
    docker run -d --name "$NAME" --privileged --network none --cgroupns=private \
        --tmpfs /run --tmpfs /run/lock "$IMAGE" >/dev/null
    local state
    for _ in $(seq 1 50); do
        state="$(docker exec "$NAME" systemctl is-system-running 2>/dev/null || true)"
        [[ "$state" == "running" || "$state" == "degraded" ]] && break
        sleep 0.2
    done
}

start_container
docker cp "$BINARY" "$NAME:/usr/libexec/better-manager-daemon"
docker cp "$ROOT_DIR/packaging/daemon/better-greeter-watch.service" \
    "$NAME:/usr/lib/systemd/system/better-greeter-watch.service"
docker cp "$ROOT_DIR/packaging/boot-watch/gdm.service" "$NAME:/etc/systemd/system/gdm.service"
docker cp "$ROOT_DIR/packaging/boot-watch/check.sh" "$NAME:/check.sh"
docker exec -e BETTER_OS_E2E_CONTAINER=1 "$NAME" bash /check.sh

[[ -n "$SUPERVISOR" ]] || exit 0
start_container
docker exec "$NAME" mkdir -p /login
docker cp "$SUPERVISOR" "$NAME:/login/better-os-login-supervisor"
docker cp "$STATUS" "$NAME:/login/better-os-login-status"
docker cp "$ROOT_DIR/packaging/greeter" "$NAME:/login/greeter"
docker cp "$ROOT_DIR/packaging/boot-watch/login-path.sh" "$NAME:/login-path.sh"
docker exec -e BETTER_OS_E2E_CONTAINER=1 "$NAME" bash /login-path.sh
