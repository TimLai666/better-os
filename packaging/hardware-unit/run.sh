#!/usr/bin/env bash
# Builds the image once (it needs the network for apt), then runs the check in
# a fresh container with no network and removes it. Never run check.sh on a
# machine you care about; this script only ever runs it in the container.
#
#   packaging/hardware-unit/run.sh path/to/better-manager-daemon
set -euo pipefail
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
BINARY="${1:?the better-manager-daemon binary to test}"
IMAGE=better-os-hardware-unit:24.04
NAME="better-os-hardware-unit-$$"

docker image inspect "$IMAGE" >/dev/null 2>&1 \
    || docker build -t "$IMAGE" "$ROOT_DIR/packaging/hardware-unit"

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
docker cp "$ROOT_DIR/packaging/daemon/better-manager-daemon.service" \
    "$NAME:/usr/lib/systemd/system/better-manager-daemon.service"
docker cp "$ROOT_DIR/packaging/daemon/better-hardware-restore.service" \
    "$NAME:/usr/lib/systemd/system/better-hardware-restore.service"
docker cp "$ROOT_DIR/packaging/hardware-unit/check.sh" "$NAME:/check.sh"
docker exec -e BETTER_OS_E2E_CONTAINER=1 "$NAME" bash /check.sh
