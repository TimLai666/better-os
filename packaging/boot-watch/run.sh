#!/usr/bin/env bash
# Builds the image once (it needs the network for apt), then runs the check in
# a fresh container with no network and removes it. Never run check.sh on a
# machine you care about: it restarts a unit named gdm.service.
#
#   packaging/boot-watch/run.sh path/to/better-manager-daemon
set -euo pipefail
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
BINARY="${1:?the better-manager-daemon binary to test}"
IMAGE=better-os-boot-watch:24.04
NAME="better-os-boot-watch-$$"

docker image inspect "$IMAGE" >/dev/null 2>&1 \
    || docker build -t "$IMAGE" "$ROOT_DIR/packaging/boot-watch"

cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT
docker run -d --name "$NAME" --privileged --network none --cgroupns=private \
    --tmpfs /run --tmpfs /run/lock "$IMAGE" >/dev/null
for _ in $(seq 1 50); do
    state="$(docker exec "$NAME" systemctl is-system-running 2>/dev/null || true)"
    [[ "$state" == "running" || "$state" == "degraded" ]] && break
    sleep 0.2
done
docker cp "$BINARY" "$NAME:/usr/libexec/better-manager-daemon"
docker cp "$ROOT_DIR/packaging/daemon/better-greeter-watch.service" \
    "$NAME:/usr/lib/systemd/system/better-greeter-watch.service"
docker cp "$ROOT_DIR/packaging/boot-watch/gdm.service" "$NAME:/etc/systemd/system/gdm.service"
docker cp "$ROOT_DIR/packaging/boot-watch/check.sh" "$NAME:/check.sh"
docker exec -e BETTER_OS_E2E_CONTAINER=1 "$NAME" bash /check.sh
