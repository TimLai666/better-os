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
# The wire contract wants the bare release, the packaging contract wants the
# `ubuntu-` prefixed target.
RELEASE_ID="${RELEASE_TARGET#ubuntu-}"
if [[ -z "$RELEASE_ID" ]]; then
    RELEASE_ID="$(sed -n 's/^VERSION_ID="\?\([^"]*\)"\?/\1/p' /etc/os-release)"
fi

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
FIXTURE_DIR=/opt/better-os/e2e-fixtures
ROLLBACK_COMPONENT=better-rollback-fixture
ROLLBACK_OLD_DEB="$FIXTURE_DIR/${ROLLBACK_COMPONENT}_0.0.9_${RELEASE_TARGET}_${ARCH}.deb"
ROLLBACK_NEW_DEB="$FIXTURE_DIR/${ROLLBACK_COMPONENT}_0.1.0_${RELEASE_TARGET}_${ARCH}.deb"

assert_completed_journal() {
    local outcome="$1"
    local transaction_id
    transaction_id="$(printf '%s' "$outcome" | sed -n 's/.*"transaction_id":"\([^"]*\)".*/\1/p')"
    [[ -n "$transaction_id" ]] || {
        printf 'The outcome did not contain a transaction id\n' >&2
        exit 1
    }
    local journal="/var/lib/better-os/transactions/$transaction_id.json"
    [[ -f "$journal" ]] || {
        printf 'Missing journal %s\n' "$journal" >&2
        exit 1
    }
    grep -q '"state":"completed"' "$journal" || {
        printf 'Journal %s did not reach completed\n' "$journal" >&2
        exit 1
    }
}

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
    /var/lib/better-os/installed \
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

# One of the packages added in ticket 36, installed and removed the ordinary
# way. better-launcher is the cheap one to carry: its runtime dependencies are
# the same set the image already installs for better-monitor, so this needs no
# new base image, no network at test time, and no new infrastructure. The other
# new packages are covered by verify-deb.sh assertions rather than here.
printf '== a package added in ticket 36 installs and removes through apt ==\n'
LAUNCHER_DEB="$(find_package better-launcher)"
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$LAUNCHER_DEB"
dpkg-query -W -f='${db:Status-Status}' better-launcher | grep -q '^installed$' || {
    printf 'better-launcher did not install\n' >&2
    exit 1
}
for required_file in \
    /usr/bin/better-launcher \
    /usr/share/applications/better-launcher.desktop \
    /usr/share/doc/better-launcher/copyright; do
    [[ -s "$required_file" ]] || {
        printf 'Missing %s after installing better-launcher\n' "$required_file" >&2
        exit 1
    }
done
# THIRD-PARTY-LICENSES.md is in the package — verify-deb.sh asserts it from the
# archive — but minimized Ubuntu images ship a dpkg path-exclude for
# /usr/share/doc/* (keeping only copyright), so dpkg drops it at unpack here.
# Asserting its absence-or-presence would test the image's dpkg config, not the
# package, so the payload check stays in verify-deb.sh.
# The dependency metadata has to be enough on its own. If the binary cannot
# resolve its libraries here, in an image with no *-dev packages, then the
# declared Depends is wrong no matter what the control file says.
if ldd /usr/bin/better-launcher | grep -q 'not found'; then
    printf 'better-launcher has an unresolved library after a clean apt install\n' >&2
    exit 1
fi
DEBIAN_FRONTEND=noninteractive apt-get remove -y better-launcher
if dpkg-query -W -f='${db:Status-Status}' better-launcher 2>/dev/null | grep -q '^installed$'; then
    printf 'better-launcher survived removal\n' >&2
    exit 1
fi
[[ ! -e /usr/bin/better-launcher ]] || {
    printf 'better-launcher left its binary behind after removal\n' >&2
    exit 1
}
[[ ! -e /usr/share/applications/better-launcher.desktop ]] || {
    printf 'better-launcher left its desktop entry behind after removal\n' >&2
    exit 1
}
printf 'better-launcher installed and removed cleanly\n'

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

printf '== the authorized path, end to end ==\n'
# Everything above proves the service refuses what it should. This proves it
# does what it should when polkit says yes — which until now had only ever run
# against a fake authorizer, along with the real APT driver, the real health
# check, and the real rollback.
CLIENT=/opt/better-os/e2e_client
if [[ ! -x "$CLIENT" ]]; then
    printf 'Missing the end-to-end client at %s\n' "$CLIENT" >&2
    exit 1
fi

# The test-only polkit authorization, dropped in now so the refusal checks
# above ran without it.
install -m 0644 /opt/better-os/e2e/50-better-os-e2e.rules \
    /etc/polkit-1/rules.d/50-better-os-e2e.rules 2>/dev/null || true
mkdir -p /etc/polkit-1/localauthority/50-local.d
install -m 0644 /opt/better-os/e2e/50-better-os-e2e.pkla \
    /etc/polkit-1/localauthority/50-local.d/50-better-os-e2e.pkla 2>/dev/null || true
# polkit reloads its policy on its own, but not instantly.
sleep 2

printf '== a real failed install is rolled back by removing the package ==\n'
[[ -f "$ROLLBACK_OLD_DEB" && -f "$ROLLBACK_NEW_DEB" ]] || {
    printf 'The rollback fixture packages were not built in the container\n' >&2
    exit 1
}
if dpkg-query -W -f='${db:Status-Status}' "$ROLLBACK_COMPONENT" 2>/dev/null | grep -q '^installed$'; then
    DEBIAN_FRONTEND=noninteractive apt-get remove -y "$ROLLBACK_COMPONENT" >/dev/null
fi
[[ ! -e "/var/lib/better-os/installed/$ROLLBACK_COMPONENT.json" ]] || {
    printf 'The fresh-install fixture already has an installed artifact record\n' >&2
    exit 1
}

if outcome="$("$CLIENT" install "$RELEASE_ID" "$ARCH" "$ROLLBACK_NEW_DEB" 2>/tmp/rollback-fresh.stderr)"; then
    printf 'The unhealthy fresh install unexpectedly succeeded\n' >&2
    exit 1
fi
printf 'fresh-install outcome: %s\n' "$outcome"
printf '%s' "$outcome" | grep -q '"state":"failed"' || {
    printf 'The unhealthy fresh install did not fail\n' >&2
    exit 1
}
printf '%s' "$outcome" | grep -q '"error_key":"daemon.error.health_failed:'"$ROLLBACK_COMPONENT"'"' || {
    printf 'The fresh failure did not report the health error key\n' >&2
    exit 1
}
printf '%s' "$outcome" | grep -q '"recovery":"restored"' || {
    printf 'The fresh failure was not reported as restored\n' >&2
    exit 1
}
printf '%s' "$outcome" | grep -q '"applied_version":"0.1.0"' || {
    printf 'The fresh failure did not prove the unhealthy package was applied\n' >&2
    exit 1
}
assert_completed_journal "$outcome"
if dpkg-query -W -f='${db:Status-Status}' "$ROLLBACK_COMPONENT" 2>/dev/null | grep -q '^installed$'; then
    printf 'The failed fresh install left the package installed\n' >&2
    exit 1
fi
[[ ! -e "/var/lib/better-os/installed/$ROLLBACK_COMPONENT.json" ]] || {
    printf 'The failed fresh install left an installed artifact record\n' >&2
    exit 1
}
printf 'The real failed install removed the package and its record\n'

printf '== a real failed update reinstalls the previous artifact ==\n'
old_fixture_sha256="$(sha256sum "$ROLLBACK_OLD_DEB" | awk '{print $1}')"
outcome="$("$CLIENT" install "$RELEASE_ID" "$ARCH" "$ROLLBACK_OLD_DEB")" || {
    printf 'The healthy rollback fixture install was refused\n' >&2
    exit 1
}
printf '%s' "$outcome" | grep -q '"state":"succeeded"' || {
    printf 'The healthy rollback fixture did not install\n' >&2
    exit 1
}
dpkg-query -W -f='${db:Status-Status} ${Version}' "$ROLLBACK_COMPONENT" | grep -q '^installed 0.0.9$' || {
    printf 'The healthy rollback fixture is not installed at 0.0.9\n' >&2
    exit 1
}
installed_record="/var/lib/better-os/installed/$ROLLBACK_COMPONENT.json"
grep -q '"version":"0.0.9"' "$installed_record" || {
    printf 'The successful install did not persist version 0.0.9\n' >&2
    exit 1
}
grep -q '"filename":"'"$(basename "$ROLLBACK_OLD_DEB")"'"' "$installed_record" || {
    printf 'The successful install did not persist the old filename\n' >&2
    exit 1
}
grep -q '"sha256":"'"$old_fixture_sha256"'"' "$installed_record" || {
    printf 'The successful install did not persist the old checksum\n' >&2
    exit 1
}
assert_completed_journal "$outcome"

if outcome="$("$CLIENT" update "$RELEASE_ID" "$ARCH" 0.0.9 "$ROLLBACK_NEW_DEB" 2>/tmp/rollback-update.stderr)"; then
    printf 'The unhealthy update unexpectedly succeeded\n' >&2
    exit 1
fi
printf 'failed-update outcome: %s\n' "$outcome"
printf '%s' "$outcome" | grep -q '"state":"failed"' || {
    printf 'The unhealthy update did not fail\n' >&2
    exit 1
}
printf '%s' "$outcome" | grep -q '"error_key":"daemon.error.health_failed:'"$ROLLBACK_COMPONENT"'"' || {
    printf 'The update failure did not report the health error key\n' >&2
    exit 1
}
printf '%s' "$outcome" | grep -q '"recovery":"restored"' || {
    printf 'The update failure was not reported as restored\n' >&2
    exit 1
}
printf '%s' "$outcome" | grep -q '"applied_version":"0.1.0"' || {
    printf 'The update failure did not prove the unhealthy package was applied\n' >&2
    exit 1
}
assert_completed_journal "$outcome"
dpkg-query -W -f='${db:Status-Status} ${Version}' "$ROLLBACK_COMPONENT" | grep -q '^installed 0.0.9$' || {
    printf 'Rollback did not restore the old dpkg version\n' >&2
    exit 1
}
grep -q '"filename":"'"$(basename "$ROLLBACK_OLD_DEB")"'"' "$installed_record" || {
    printf 'Rollback did not restore the old installed artifact record\n' >&2
    exit 1
}
if grep -q '"filename":"'"$(basename "$ROLLBACK_NEW_DEB")"'"' "$installed_record"; then
    printf 'Rollback left the new artifact in the installed record\n' >&2
    exit 1
fi
printf 'The real failed update restored dpkg and the old artifact record\n'

outcome="$("$CLIENT" remove "$RELEASE_ID" "$ARCH" "$ROLLBACK_COMPONENT" 0.0.9)" || {
    printf 'The rollback fixture cleanup was refused\n' >&2
    exit 1
}
assert_completed_journal "$outcome"
if dpkg-query -W -f='${db:Status-Status}' "$ROLLBACK_COMPONENT" 2>/dev/null | grep -q '^installed$'; then
    printf 'The rollback fixture cleanup left the package installed\n' >&2
    exit 1
fi
[[ ! -e "$installed_record" ]] || {
    printf 'The removal did not clear the installed artifact record\n' >&2
    exit 1
}
printf 'The rollback fixture cleanup removed the package and its record\n'

# Start from a machine that does not have the component, so a successful
# install is visible rather than assumed.
if dpkg-query -W -f='${db:Status-Status}' better-monitor 2>/dev/null | grep -q '^installed$'; then
    DEBIAN_FRONTEND=noninteractive apt-get remove -y better-monitor >/dev/null
fi

MONITOR_ASSET="$(basename "$MONITOR_DEB")"
cp "$MONITOR_DEB" "/tmp/$MONITOR_ASSET"

printf 'Installing %s through the privileged service\n' "$MONITOR_ASSET"
outcome="$("$CLIENT" install "$RELEASE_ID" "$ARCH" "/tmp/$MONITOR_ASSET")" || {
    printf 'The authorized install was refused\n' >&2
    exit 1
}
printf 'outcome: %s\n' "$outcome"

printf '%s' "$outcome" | grep -q '"state":"succeeded"' || {
    printf 'The transaction did not succeed\n' >&2
    exit 1
}
printf '%s' "$outcome" | grep -q '"state":"healthy"' || {
    printf 'The service did not report a healthy component\n' >&2
    exit 1
}

# The claim that matters: dpkg, not the outcome document, says it is installed.
installed_now="$(dpkg-query -W -f='${db:Status-Status} ${Version}' better-monitor)"
printf 'dpkg reports: %s\n' "$installed_now"
printf '%s' "$installed_now" | grep -q '^installed ' || {
    printf 'The service reported success but dpkg disagrees\n' >&2
    exit 1
}
applied_version="${installed_now##* }"
printf '%s' "$outcome" | grep -q "\"applied_version\":\"$applied_version\"" || {
    printf 'The reported version does not match what dpkg installed\n' >&2
    exit 1
}
printf 'The service installed the component for real\n'

# A completed transaction has to be readable afterwards, which is what a client
# that lost its connection depends on.
[[ -n "$(ls -A /var/lib/better-os/transactions 2>/dev/null)" ]] || {
    printf 'A completed transaction left no journal\n' >&2
    exit 1
}
grep -q 'completed' /var/lib/better-os/transactions/*.json || {
    printf 'The journal does not record a completed transaction\n' >&2
    exit 1
}
# The component was absent before, so undoing this install means removing it.
grep -q '"component":"better-monitor"' /var/lib/better-os/rollback/better-monitor.json || {
    printf 'No rollback record was written for the installed component\n' >&2
    exit 1
}
printf 'The transaction journal and rollback record are on disk\n'

printf 'Removing %s through the privileged service\n' better-monitor
outcome="$("$CLIENT" remove "$RELEASE_ID" "$ARCH" better-monitor "$applied_version")" || {
    printf 'The authorized removal was refused\n' >&2
    exit 1
}
printf '%s' "$outcome" | grep -q '"state":"succeeded"' || {
    printf 'The removal transaction did not succeed\n' >&2
    exit 1
}
if dpkg-query -W -f='${db:Status-Status}' better-monitor 2>/dev/null | grep -q '^installed$'; then
    printf 'The service reported a removal but the package is still installed\n' >&2
    exit 1
fi
printf 'The service removed the component for real\n'

printf '== a host that moved since planning is refused ==\n'
# Reinstall, then claim a prior version that is not what dpkg has. The service
# must refuse rather than overwrite a state nobody reviewed.
"$CLIENT" install "$RELEASE_ID" "$ARCH" "/tmp/$MONITOR_ASSET" >/dev/null
if "$CLIENT" remove "$RELEASE_ID" "$ARCH" better-monitor 9.9.9 >/dev/null 2>&1; then
    printf 'The service acted on a plan whose expected version was wrong\n' >&2
    exit 1
fi
dpkg-query -W -f='${db:Status-Status}' better-monitor | grep -q '^installed$' || {
    printf 'A refused transaction removed the package anyway\n' >&2
    exit 1
}
printf 'The service refused a plan that disagreed with dpkg, and changed nothing\n'

DEBIAN_FRONTEND=noninteractive apt-get remove -y better-monitor >/dev/null

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
