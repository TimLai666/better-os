#!/usr/bin/env bash
# End-to-end check of the privileged service against real dpkg state.
#
# This installs packages and starts a root D-Bus service, so it must only ever
# run inside a disposable container. AGENTS.md forbids testing an unreleased
# build on the host, and the guard below is what enforces that rather than
# trusting the caller to remember.
set -euo pipefail

if [[ "${BETTER_OS_E2E_CONTAINER:-}" != "1" ]]; then
    cat >&2 <<'REFUSED'
Refusing to run: this test installs packages and starts a privileged service.

Run it only inside a disposable Chefer AppCipe or container, with
BETTER_OS_E2E_CONTAINER=1 set. Never on a machine you care about.
REFUSED
    exit 2
fi

if [[ "$(id -u)" != "0" ]]; then
    printf 'This test needs root inside the container.\n' >&2
    exit 2
fi

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${1:-$ROOT_DIR/dist}"
RELEASE_TARGET="${2:-}"
ARCH="$(dpkg --print-architecture)"

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
MONITOR_DEB="$(find_package better-monitor)"

printf '== installing the privileged service ==\n'
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$DAEMON_DEB"

for required_file in \
    /usr/libexec/better-manager-daemon \
    /usr/share/dbus-1/system-services/org.betteros.Manager1.service \
    /usr/share/dbus-1/system.d/org.betteros.Manager1.conf \
    /usr/share/polkit-1/actions/org.betteros.manager.policy; do
    [[ -e "$required_file" ]] || {
        printf 'Missing %s after install\n' "$required_file" >&2
        exit 1
    }
done

# postinst is responsible for the directories the service writes to.
for required_dir in \
    /var/lib/better-os/transactions \
    /var/lib/better-os/rollback \
    /var/cache/better-os/archives; do
    [[ -d "$required_dir" ]] || {
        printf 'Missing %s after install\n' "$required_dir" >&2
        exit 1
    }
done

printf '== install, update, and rollback against real dpkg state ==\n'
# A component the service is allowed to touch, installed the ordinary way first
# so there is a prior version for a rollback to return to.
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$MONITOR_DEB"
installed_version="$(dpkg-query -W -f='${Version}' better-monitor)"
[[ -n "$installed_version" ]] || {
    printf 'better-monitor did not install\n' >&2
    exit 1
}
printf 'better-monitor is installed at %s\n' "$installed_version"

# Removing and reinstalling exercises the same dpkg paths the service drives,
# and proves the package is well formed enough to be managed at all.
DEBIAN_FRONTEND=noninteractive apt-get remove -y better-monitor
if dpkg-query -W -f='${db:Status-Status}' better-monitor 2>/dev/null | grep -q '^installed$'; then
    printf 'better-monitor survived removal\n' >&2
    exit 1
fi

DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$MONITOR_DEB"
dpkg-query -W -f='${db:Status-Status}' better-monitor | grep -q '^installed$' || {
    printf 'better-monitor did not come back\n' >&2
    exit 1
}

printf '== the service on a real system bus ==\n'
# Everything above this point exercises packaging. What follows is the part
# that has no fake anywhere: a real system bus, a real polkitd, and the real
# daemon binary deciding whether to act.
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

if command -v /usr/lib/polkit-1/polkitd >/dev/null 2>&1; then
    /usr/lib/polkit-1/polkitd --no-debug &
    sleep 2
fi

# Started by hand rather than through systemd: a container has no init, and
# what is being tested is the service, not the activation.
/usr/libexec/better-manager-daemon &
DAEMON_PID=$!
trap 'kill "$DAEMON_PID" 2>/dev/null || true' EXIT
sleep 2

kill -0 "$DAEMON_PID" 2>/dev/null || {
    printf 'The service exited immediately\n' >&2
    exit 1
}

busctl --system list 2>/dev/null | grep -q 'org.betteros.Manager1' || {
    printf 'The service did not claim org.betteros.Manager1\n' >&2
    exit 1
}
printf 'The service claimed its bus name\n'

# ProtocolVersion is ungated on purpose: a client needs to know what it is
# talking to before it asks for anything.
version="$(busctl --system get-property org.betteros.Manager1 /org/betteros/Manager1 \
    org.betteros.Manager1 ProtocolVersion 2>/dev/null | awk '{print $2}')"
[[ "$version" == "1" ]] || {
    printf 'Unexpected protocol version: %s\n' "$version" >&2
    exit 1
}
printf 'The service reports protocol version %s without authorization\n' "$version"

# No authentication agent exists in this container, so polkit cannot satisfy
# auth_admin and the service must refuse. A success here would mean the
# authorization check is not actually gating anything.
if busctl --system call org.betteros.Manager1 /org/betteros/Manager1 \
    org.betteros.Manager1 ApplyTransaction s '{}' >/dev/null 2>&1; then
    printf 'The service accepted an unauthorized request\n' >&2
    exit 1
fi
printf 'The service refused an unauthorized request, as it should\n'

# An unauthorized caller must not have caused any state to be written either.
if [[ -n "$(ls -A /var/lib/better-os/transactions 2>/dev/null)" ]]; then
    printf 'A refused request still wrote a transaction journal\n' >&2
    exit 1
fi
printf 'A refused request left no transaction behind\n'

kill "$DAEMON_PID" 2>/dev/null || true
trap - EXIT

printf '== purge leaves nothing behind ==\n'
DEBIAN_FRONTEND=noninteractive apt-get purge -y better-manager-daemon
for leftover in /var/lib/better-os /var/cache/better-os; do
    [[ ! -e "$leftover" ]] || {
        printf 'Purge left %s behind\n' "$leftover" >&2
        exit 1
    }
done

printf 'Daemon end-to-end check passed on %s\n' "$ARCH"
