#!/usr/bin/env bash
# End-to-end check for Better Monitor's privileged DMI read helper.
#
# This installs a root service and a test-only polkit rule, so it is guarded in
# exactly the same way as the Manager transaction E2E and may run only inside a
# disposable container.
set -euo pipefail

if [[ "${BETTER_OS_E2E_CONTAINER:-}" != "1" ]]; then
    printf 'Refusing to run the Monitor DMI E2E outside its disposable container.\n' >&2
    exit 2
fi

if [[ "$(id -u)" != "0" ]]; then
    printf 'The Monitor DMI E2E needs root inside the container.\n' >&2
    exit 2
fi

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${1:-$ROOT_DIR/dist}"
RELEASE_TARGET="${2:-}"
ARCH="$(dpkg --print-architecture)"
POLKIT_RULE_SOURCE=/opt/better-os/e2e/40-better-monitor-e2e.rules
POLKIT_RULE_TARGET=/etc/polkit-1/rules.d/40-better-monitor-e2e.rules
DAEMON_PID=""
POLKIT_PID=""

cleanup() {
    rm -f "$POLKIT_RULE_TARGET"
    if [[ -n "$DAEMON_PID" ]]; then
        kill "$DAEMON_PID" 2>/dev/null || true
        wait "$DAEMON_PID" 2>/dev/null || true
    fi
    if [[ -n "$POLKIT_PID" ]]; then
        kill "$POLKIT_PID" 2>/dev/null || true
        wait "$POLKIT_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

find_package() {
    local name="$1"
    local paths
    if [[ -n "$RELEASE_TARGET" ]]; then
        paths=("$DIST_DIR/${name}_"*"_${RELEASE_TARGET}_${ARCH}.deb")
    else
        paths=("$DIST_DIR/${name}_"*.deb)
    fi
    if [[ ${#paths[@]} -ne 1 || ! -f "${paths[0]}" ]]; then
        printf 'Expected exactly one %s package in %s\n' "$name" "$DIST_DIR" >&2
        exit 1
    fi
    printf '%s\n' "${paths[0]}"
}

DAEMON_DEB="$(find_package better-manager-daemon)"
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$DAEMON_DEB" >/dev/null

for required_file in \
    /usr/libexec/better-manager-daemon \
    /usr/share/dbus-1/system-services/org.betteros.Monitor1.service \
    /usr/share/dbus-1/system.d/org.betteros.Monitor1.conf \
    /usr/share/polkit-1/actions/org.betteros.manager.policy; do
    [[ -s "$required_file" ]] || {
        printf 'Missing %s after installing the privileged service\n' "$required_file" >&2
        exit 1
    }
done

grep -q 'org.betteros.monitor.read-memory-devices' \
    /usr/share/polkit-1/actions/org.betteros.manager.policy || {
    printf 'The installed policy does not declare the DMI read action\n' >&2
    exit 1
}

mkdir -p /run/dbus
if [[ ! -S /run/dbus/system_bus_socket ]]; then
    dbus-daemon --system --fork
fi
for _ in $(seq 1 50); do
    [[ -S /run/dbus/system_bus_socket ]] && break
    sleep 0.1
done
[[ -S /run/dbus/system_bus_socket ]] || {
    printf 'The system bus did not start\n' >&2
    exit 1
}

if [[ -x /usr/lib/polkit-1/polkitd ]]; then
    /usr/lib/polkit-1/polkitd --no-debug &
    POLKIT_PID=$!
    sleep 2
fi

/usr/libexec/better-manager-daemon &
DAEMON_PID=$!
for _ in $(seq 1 50); do
    if busctl --system list 2>/dev/null | grep -q 'org.betteros.Monitor1'; then
        break
    fi
    sleep 0.1
done
kill -0 "$DAEMON_PID" 2>/dev/null || {
    printf 'The privileged service exited before claiming Monitor1\n' >&2
    exit 1
}
busctl --system list 2>/dev/null | grep -q 'org.betteros.Monitor1' || {
    printf 'The service did not claim org.betteros.Monitor1\n' >&2
    exit 1
}

version="$(busctl --system get-property org.betteros.Monitor1 /org/betteros/Monitor1 \
    org.betteros.Monitor1 ProtocolVersion 2>/dev/null | awk '{print $2}')"
[[ "$version" == "1" ]] || {
    printf 'Unexpected Monitor1 protocol version: %s\n' "$version" >&2
    exit 1
}
printf 'Monitor1 reports protocol version %s before authorization\n' "$version"

# Root is an administrator in polkit and may be authorized without a prompt,
# so it is not a meaningful subject for the denial check. The system bus policy
# permits an ordinary process to address Monitor1; polkit must still reject the
# method for an unprivileged, inactive caller.
set +e
unauthorized_output="$(runuser -u nobody -- busctl --system call \
    org.betteros.Monitor1 /org/betteros/Monitor1 \
    org.betteros.Monitor1 ReadMemoryDevices 2>&1)"
unauthorized_status=$?
set -e
if [[ $unauthorized_status -eq 0 ]]; then
    printf 'Monitor1 accepted a DMI read from an unprivileged caller\n' >&2
    exit 1
fi
printf '%s\n' "$unauthorized_output" | grep -q 'daemon.error.unauthorized' || {
    printf 'Monitor1 failed before authorization for the wrong reason: %s\n' \
        "$unauthorized_output" >&2
    exit 1
}
printf 'Monitor1 refused the unprivileged DMI read\n'

[[ -s "$POLKIT_RULE_SOURCE" ]] || {
    printf 'Missing the container-only Monitor DMI polkit rule\n' >&2
    exit 1
}
install -m 0644 "$POLKIT_RULE_SOURCE" "$POLKIT_RULE_TARGET"
sleep 2

set +e
authorized_output="$(busctl --system call org.betteros.Monitor1 /org/betteros/Monitor1 \
    org.betteros.Monitor1 ReadMemoryDevices 2>&1)"
authorized_status=$?
set -e

if [[ $authorized_status -eq 0 ]]; then
    printf '%s\n' "$authorized_output" | grep -q 'protocol_version' || {
        printf 'The authorized DMI reply was not a typed report: %s\n' \
            "$authorized_output" >&2
        exit 1
    }
    if printf '%s\n' "$authorized_output" | grep -Eqi 'serial|asset[_ -]?tag|smbios_entry_point|/sys/firmware'; then
        printf 'The authorized DMI reply exposed a forbidden field or path\n' >&2
        exit 1
    fi
    printf 'Monitor1 returned a bounded, privacy-minimized DMI report\n'
else
    printf '%s\n' "$authorized_output" | grep -q 'daemon.error.host_unreadable' || {
        printf 'The authorized DMI call did not reach the host reader: %s\n' \
            "$authorized_output" >&2
        exit 1
    }
    printf 'Monitor1 passed authorization and reported that the container exposes no SMBIOS tables\n'
fi
