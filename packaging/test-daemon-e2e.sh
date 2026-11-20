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
# better-awake is here for its shape rather than for its features: it ships
# better-awake-service, awake-tray, and awake-gui and no /usr/bin/better-awake.
# The health check used to derive that one path from the package name, so every
# install of this package failed its check and was rolled back on real machines.
# It costs nothing extra to carry: dpkg-shlibdeps gives it the same graphics
# libraries the image already installs for better-monitor and better-launcher,
# so no new base image and no network at test time.
AWAKE_DEB="$(find_package better-awake)"
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
# No package here ships a postinst that runs update-desktop-database or
# gtk-update-icon-cache, and none needs one. desktop-file-utils registers
# `interest-noawait /usr/share/applications` and hicolor-icon-theme registers
# `interest-noawait /usr/share/icons/hicolor`, so dpkg refreshes both caches by
# trigger after any package drops a file into either directory. Verified on the
# Zorin 18 / noble host by reading /var/lib/dpkg/info/desktop-file-utils.triggers
# and hicolor-icon-theme.triggers, whose postinsts run update-desktop-database
# and gtk-update-icon-cache respectively. A package doing it itself would run
# the same commands a second time, and would fail on a machine where those
# packages are absent. That is why the checks below assert the installed files
# and not a refreshed cache: this image has neither package, so no trigger runs
# here, and asserting a cache would be testing the image rather than the .deb.
for required_file in \
    /usr/bin/better-launcher \
    /usr/share/applications/better-launcher.desktop \
    /usr/share/icons/hicolor/scalable/apps/better-launcher.svg \
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
[[ ! -e /usr/share/icons/hicolor/scalable/apps/better-launcher.svg ]] || {
    printf 'better-launcher left its icon behind after removal\n' >&2
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

# better-storage, installed, started, and removed (ticket 31). It needs the
# system bus above because the service is a UDisks2 client before it is
# anything else: it connects to UDisks2, then owns org.betteros.Storage1 on a
# session bus. There is no init here, so udisksd and a session bus are started
# by hand, the same way the privileged service is below. udisksd sees no
# drives in a container, which is the point: the service must still start,
# report an empty inventory, and leave nothing behind.
printf '== better-storage installs, starts, and removes cleanly ==\n'
STORAGE_DEB="$(find_package better-storage)"
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$STORAGE_DEB"
dpkg-query -W -f='${db:Status-Status}' better-storage | grep -q '^installed$' || {
    printf 'better-storage did not install\n' >&2
    exit 1
}
STORAGE_FILES=(
    /usr/bin/better-storage-service
    /usr/bin/better-storage-doctor
    /usr/lib/systemd/user/better-storage.service
    /usr/share/dbus-1/services/org.betteros.Storage1.service
)
for required_file in "${STORAGE_FILES[@]}"; do
    [[ -s "$required_file" ]] || {
        printf 'Missing %s after installing better-storage\n' "$required_file" >&2
        exit 1
    }
done
if ldd /usr/bin/better-storage-service | grep -q 'not found'; then
    printf 'better-storage-service has an unresolved library after a clean apt install\n' >&2
    exit 1
fi

UDISKSD=/usr/libexec/udisks2/udisksd
[[ -x "$UDISKSD" ]] || {
    printf 'udisks2 is a Depends of better-storage and %s is missing\n' "$UDISKSD" >&2
    exit 1
}
"$UDISKSD" --no-debug >/tmp/udisksd.log 2>&1 &
UDISKSD_PID=$!
for _ in $(seq 1 50); do
    busctl --system list 2>/dev/null | grep -q 'org.freedesktop.UDisks2' && break
    sleep 0.1
done
busctl --system list 2>/dev/null | grep -q 'org.freedesktop.UDisks2' || {
    printf 'udisksd did not claim org.freedesktop.UDisks2\n' >&2
    cat /tmp/udisksd.log >&2 || true
    exit 1
}

STORAGE_HOME="$(mktemp -d)"
SESSION_BUS_ADDRESS="$(dbus-daemon --session --fork --print-address=1 --print-pid=3 3>/tmp/session-bus.pid)"
SESSION_BUS_PID="$(cat /tmp/session-bus.pid)"
HOME="$STORAGE_HOME" XDG_CONFIG_HOME="$STORAGE_HOME/.config" \
    DBUS_SESSION_BUS_ADDRESS="$SESSION_BUS_ADDRESS" \
    /usr/bin/better-storage-service >/tmp/storage-service.log 2>&1 &
STORAGE_PID=$!
for _ in $(seq 1 50); do
    busctl --address="$SESSION_BUS_ADDRESS" list 2>/dev/null | grep -q 'org.betteros.Storage1' && break
    sleep 0.1
done
busctl --address="$SESSION_BUS_ADDRESS" list 2>/dev/null | grep -q 'org.betteros.Storage1' || {
    printf 'better-storage-service did not claim org.betteros.Storage1\n' >&2
    cat /tmp/storage-service.log >&2 || true
    exit 1
}
kill -0 "$STORAGE_PID" 2>/dev/null || {
    printf 'better-storage-service exited after claiming its name\n' >&2
    exit 1
}
printf 'better-storage-service is running and owns org.betteros.Storage1\n'

# Better Manager stops the service before it removes the package; dpkg never
# stops a user service. This stands in for that step.
# systemd stops a unit with SIGTERM; a background job of a non-interactive
# shell starts with SIGINT ignored, so SIGINT here would test the shell.
kill -TERM "$STORAGE_PID"
for _ in $(seq 1 50); do
    kill -0 "$STORAGE_PID" 2>/dev/null || break
    sleep 0.1
done
if kill -0 "$STORAGE_PID" 2>/dev/null; then
    printf 'better-storage-service did not stop on SIGTERM\n' >&2
    cat /tmp/storage-service.log >&2 || true
    exit 1
fi
DEBIAN_FRONTEND=noninteractive apt-get remove -y better-storage
if dpkg-query -W -f='${db:Status-Status}' better-storage 2>/dev/null | grep -q '^installed$'; then
    printf 'better-storage survived removal\n' >&2
    exit 1
fi
for leftover in "${STORAGE_FILES[@]}"; do
    [[ ! -e "$leftover" ]] || {
        printf 'better-storage left %s behind after removal\n' "$leftover" >&2
        exit 1
    }
done
# comm is cut to 15 characters, so match the command line instead.
if pgrep -f '^/usr/bin/better-storage-service' >/dev/null 2>&1; then
    printf 'a better-storage-service process outlived the package\n' >&2
    exit 1
fi
# Starting with no device must write no state: the preference file is only
# written when a person changes a device's mode. Deleting the person's own
# data on uninstall is Better Manager's step, not dpkg's.
[[ -z "$(find "$STORAGE_HOME" -mindepth 1 -print -quit)" ]] || {
    printf 'better-storage-service wrote state at start with no device:\n' >&2
    find "$STORAGE_HOME" -mindepth 1 >&2
    exit 1
}
kill "$SESSION_BUS_PID" "$UDISKSD_PID" 2>/dev/null || true
rm -rf "$STORAGE_HOME"
printf 'better-storage installed, started, and removed cleanly\n'

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

printf '== drive mounting choices: an fstab entry written and removed through the service ==\n'
# Ticket 127, ADR 0032. The container has no block device the service may
# touch, so a stand-in UDisks2 answers for two drives; the service, polkit,
# /etc/fstab, the udev rules file, and findmnt are the real ones. The
# internal drive reads as mounted already, so the service writes its line and
# starts no unit: there is no systemd here.
[[ -f /etc/fstab ]] || printf '# UNCONFIGURED FSTAB\n' > /etc/fstab
cp -p /etc/fstab /tmp/fstab.before
INTERNAL_UUID=e2e00001-0000-4000-8000-000000000001
STICK_UUID=E2E0-0002
DRIVE_RULES=/etc/udev/rules.d/90-better-os-drives.rules
/opt/better-os/e2e_udisks >/tmp/e2e-udisks.log 2>&1 &
E2E_UDISKS_PID=$!
for _ in $(seq 1 50); do
    busctl --system list 2>/dev/null | grep -q 'org.freedesktop.UDisks2' && break
    sleep 0.1
done
busctl --system list 2>/dev/null | grep -q 'org.freedesktop.UDisks2' || {
    printf 'The stand-in UDisks2 did not claim its name\n' >&2
    cat /tmp/e2e-udisks.log >&2 || true
    exit 1
}
# findmnt checks that every source exists, `nofail` or not, and the stand-in
# drive's UUID names no device in this container. That one error line is the
# only one allowed; any other error or warning still fails the check.
verify_fstab() {
    local report
    report="$(findmnt --verify --tab-file /etc/fstab 2>&1)" && return 0
    printf '%s\n' "$report" \
        | grep -E '^[[:space:]]*\[[EW]\]' \
        | grep -vxF "   [E] unreachable on boot required source: UUID=$INTERNAL_UUID" \
        | grep -q . && { printf '%s\n' "$report" >&2; return 1; }
    return 0
}
drive_call() {
    busctl --system call org.betteros.Manager1 /org/betteros/Manager1 \
        org.betteros.Drives1 SetMountAutomatically sb "$1" "$2"
}
output="$(drive_call "$INTERNAL_UUID" true 2>&1)" || {
    printf 'Turning the drive on failed: %s\n' "$output" >&2
    exit 1
}
entry="UUID=$INTERNAL_UUID /mnt/better-os/$INTERNAL_UUID ext4 nosuid,nodev,nofail,x-gvfs-show,x-systemd.device-timeout=10s 0 0"
grep -qxF "$entry" /etc/fstab || {
    printf 'The entry was not written as expected:\n' >&2
    cat /etc/fstab >&2
    exit 1
}
head -c "$(stat -c %s /tmp/fstab.before)" /etc/fstab | cmp -s - /tmp/fstab.before || {
    printf 'Lines Better OS did not write were changed\n' >&2
    exit 1
}
[[ "$(stat -c '%a %u %g' /etc/fstab)" == "$(stat -c '%a %u %g' /tmp/fstab.before)" ]] || {
    printf 'The fstab lost its mode or owner\n' >&2
    exit 1
}
mkdir -p "/mnt/better-os/$INTERNAL_UUID"
verify_fstab || {
    printf 'findmnt --verify refused the file with the entry\n' >&2
    exit 1
}
# A UUID with a path in it never reaches the file.
if drive_call "../../etc" true >/dev/null 2>&1; then
    printf 'The service accepted a UUID that is not one\n' >&2
    exit 1
fi
drive_call "$INTERNAL_UUID" false >/dev/null || {
    printf 'Turning the drive off failed\n' >&2
    exit 1
}
cmp -s /etc/fstab /tmp/fstab.before || {
    printf 'Turning the drive off did not give the file back as it was\n' >&2
    diff /tmp/fstab.before /etc/fstab >&2 || true
    exit 1
}
[[ ! -e "/mnt/better-os/$INTERNAL_UUID" ]] || {
    printf 'The empty mount point was left behind\n' >&2
    exit 1
}
verify_fstab || {
    printf 'findmnt --verify refused the file after the entry was removed\n' >&2
    exit 1
}
drive_call "$STICK_UUID" false >/dev/null || {
    printf 'Turning the stick off failed\n' >&2
    exit 1
}
grep -qxF "ENV{ID_FS_UUID}==\"$STICK_UUID\", ENV{UDISKS_AUTO}=\"0\"" "$DRIVE_RULES" || {
    printf 'The stick turned off has no udev rule\n' >&2
    exit 1
}
# What the package's prerm runs on removal takes every entry and rule.
drive_call "$INTERNAL_UUID" true >/dev/null
grep -q "$INTERNAL_UUID" /etc/fstab
[[ -e /run/better-os/fstab.lock ]] || {
    printf 'The service did not take its lock\n' >&2
    exit 1
}
/usr/libexec/better-manager-daemon --remove-drive-entries
cmp -s /etc/fstab /tmp/fstab.before || {
    printf 'Removing the drive entries did not give the file back as it was\n' >&2
    exit 1
}
[[ ! -e "$DRIVE_RULES" ]] || {
    printf 'Removing the drive entries left the udev rules file\n' >&2
    exit 1
}
kill "$E2E_UDISKS_PID" 2>/dev/null || true
printf 'Drive choices were written and removed, and findmnt --verify accepted the file\n'

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

printf '== a package with no binary of its own name installs and stays installed ==\n'
# The field failure of v0.2.5, reproduced against real dpkg state and the real
# health check. This package must pass its check without a /usr/bin/better-awake
# existing anywhere, and must still be installed when the transaction ends.
if dpkg-query -W -f='${db:Status-Status}' better-awake 2>/dev/null | grep -q '^installed$'; then
    DEBIAN_FRONTEND=noninteractive apt-get remove -y better-awake >/dev/null
fi
AWAKE_ASSET="$(basename "$AWAKE_DEB")"
cp "$AWAKE_DEB" "/tmp/$AWAKE_ASSET"
outcome="$("$CLIENT" install "$RELEASE_ID" "$ARCH" "/tmp/$AWAKE_ASSET")" || {
    printf 'The authorized better-awake install was refused\n' >&2
    exit 1
}
printf 'better-awake outcome: %s\n' "$outcome"
printf '%s' "$outcome" | grep -q '"state":"succeeded"' || {
    printf 'The better-awake transaction did not succeed\n' >&2
    exit 1
}
printf '%s' "$outcome" | grep -q '"state":"healthy"' || {
    printf 'better-awake did not pass its health check\n' >&2
    exit 1
}
[[ ! -e /usr/bin/better-awake ]] || {
    printf 'better-awake now ships a binary of its own name; this check no longer proves anything\n' >&2
    exit 1
}
for required_file in \
    /usr/bin/better-awake-service \
    /usr/bin/awake-tray \
    /usr/bin/awake-gui; do
    [[ -s "$required_file" ]] || {
        printf 'Missing %s after installing better-awake\n' "$required_file" >&2
        exit 1
    }
done
dpkg-query -W -f='${db:Status-Status}' better-awake | grep -q '^installed$' || {
    printf 'better-awake was rolled back despite a healthy install\n' >&2
    exit 1
}
# Every file dpkg lists is what the health check now asserts, so assert it here
# too, against the real database rather than the daemon's report of it.
#
# Except under /usr/share/doc: this image configures dpkg with
# `path-exclude=/usr/share/doc/*`, and dpkg lists path-excluded files even
# though it never unpacked them. That is why the daemon reads dpkg's own filter
# configuration before failing a package for a missing file — and it is why
# this install passing its check on this image is worth something: a health
# check that ignored those filters would fail every package here.
while read -r listed_path; do
    case "$listed_path" in
        /usr/share/doc/*) continue ;;
    esac
    [[ -e "$listed_path" || -L "$listed_path" ]] || {
        printf 'dpkg lists %s for better-awake but it is not on disk\n' "$listed_path" >&2
        exit 1
    }
done < <(dpkg-query -L better-awake | grep '^/')
# The image's filters must actually be what the comment above says, or this
# check is proving less than it claims.
grep -rqs 'path-exclude' /etc/dpkg/dpkg.cfg /etc/dpkg/dpkg.cfg.d/ || {
    printf 'This image configures no dpkg path-exclude, so the excluded-file path is untested here\n'
}
printf 'better-awake installed through the service and passed its health check\n'

outcome="$("$CLIENT" remove "$RELEASE_ID" "$ARCH" better-awake "$(dpkg-query -W -f='${Version}' better-awake)")" || {
    printf 'The authorized better-awake removal was refused\n' >&2
    exit 1
}
printf '%s' "$outcome" | grep -q '"state":"succeeded"' || {
    printf 'The better-awake removal did not succeed\n' >&2
    exit 1
}
if dpkg-query -W -f='${db:Status-Status}' better-awake 2>/dev/null | grep -q '^installed$'; then
    printf 'better-awake survived removal through the service\n' >&2
    exit 1
fi
rm -f "/tmp/$AWAKE_ASSET"
printf 'better-awake removed through the service\n'

kill "$DAEMON_PID" 2>/dev/null || true
trap - EXIT

printf '== the bootstrap installer against locally built packages ==\n'
# install.sh is the command a user pastes on a fresh machine. Its release
# resolution half needs the public GitHub API and is exercised on the CI
# runner; this is the half that changes a machine, so it belongs here, where
# the machine is disposable. --from-dir puts the packages this job just built
# where a download would have left them. Everything after that point — the
# checksum verification, the single privileged apt call, the already-current
# second run, and --uninstall — is the shipped path with nothing stubbed.
INSTALLER=/opt/better-os/install.sh
[[ -x "$INSTALLER" ]] || {
    printf 'Missing the bootstrap installer at %s\n' "$INSTALLER" >&2
    exit 1
}

installer_reports_installed() {
    dpkg-query -W -f='${db:Status-Status}' "$1" 2>/dev/null | grep -q '^installed$'
}

# Start from a machine with neither package. The daemon was installed the
# ordinary way at the top of this script, so a successful installer run would
# otherwise be indistinguishable from what was already there.
DEBIAN_FRONTEND=noninteractive apt-get remove -y better-manager-daemon >/dev/null
if installer_reports_installed better-manager-daemon || installer_reports_installed better-manager; then
    printf 'The installer test did not start from a clean machine\n' >&2
    exit 1
fi

# A dry run has to be readable and has to change nothing.
"$INSTALLER" --from-dir "$DIST_DIR" --dry-run
for package_name in better-manager better-manager-daemon; do
    if installer_reports_installed "$package_name"; then
        printf 'The installer dry run installed %s\n' "$package_name" >&2
        exit 1
    fi
done
printf 'The installer dry run changed nothing\n'

# A tampered package must be refused before apt is reached. This is the whole
# reason the checksums are downloaded at all, so it is asserted rather than
# assumed from the fact that the verification code exists.
TAMPER_DIR=/tmp/installer-tamper
rm -rf "$TAMPER_DIR"
mkdir -p "$TAMPER_DIR"
cp "$DIST_DIR"/better-manager_*.deb "$DIST_DIR"/better-manager_*.deb.sha256 \
    "$DIST_DIR"/better-manager-daemon_*.deb "$DIST_DIR"/better-manager-daemon_*.deb.sha256 \
    "$TAMPER_DIR/"
printf 'tampered\n' >> "$TAMPER_DIR/$(basename "$(find_package better-manager)")"
if "$INSTALLER" --from-dir "$TAMPER_DIR" >/tmp/installer-tamper.log 2>&1; then
    printf 'The installer accepted a package whose checksum did not match\n' >&2
    exit 1
fi
grep -q 'checksum mismatch' /tmp/installer-tamper.log || {
    printf 'The installer failed on a tampered package for the wrong reason:\n' >&2
    cat /tmp/installer-tamper.log >&2
    exit 1
}
for package_name in better-manager better-manager-daemon; do
    if installer_reports_installed "$package_name"; then
        printf 'A refused checksum still installed %s\n' "$package_name" >&2
        exit 1
    fi
done
rm -rf "$TAMPER_DIR"
printf 'The installer refused a tampered package and installed nothing\n'

# The real thing. better-shell-adapter comes with them: better-manager
# recommends it (ADR 0018), and the installer puts it beside the manager.
"$INSTALLER" --from-dir "$DIST_DIR"
for package_name in better-manager better-manager-daemon better-shell-adapter; do
    installer_reports_installed "$package_name" || {
        printf 'The installer did not install %s\n' "$package_name" >&2
        exit 1
    }
done
for required_file in \
    /usr/bin/better-manager \
    /usr/libexec/better-manager-daemon \
    /usr/libexec/better-window-attention \
    /usr/share/gnome-shell/extensions/shell-adapter@betteros.org/extension.js \
    /usr/share/polkit-1/actions/org.betteros.manager.policy; do
    [[ -s "$required_file" ]] || {
        printf 'Missing %s after the installer ran\n' "$required_file" >&2
        exit 1
    }
done
# The dependency metadata has to carry the manager on its own, in an image
# with no *-dev packages, exactly as it must on a user's machine.
if ldd /usr/bin/better-manager | grep -q 'not found'; then
    printf 'better-manager has an unresolved library after the installer ran\n' >&2
    exit 1
fi
printf 'The installer installed the manager, its service, and the shell adapter\n'

# Re-running must not reinstall. A user who pastes the command twice should be
# told the machine is current, not asked for a password again.
"$INSTALLER" --from-dir "$DIST_DIR" >/tmp/installer-rerun.log 2>&1 || {
    printf 'The second installer run failed\n' >&2
    cat /tmp/installer-rerun.log >&2
    exit 1
}
grep -q 'already installed and current' /tmp/installer-rerun.log || {
    printf 'The second installer run did not report the machine as current:\n' >&2
    cat /tmp/installer-rerun.log >&2
    exit 1
}
printf 'The installer is idempotent\n'

printf '== removing Better Manager undoes the fixes it applied (ticket 90) ==\n'
# A person whose Fixes page applied the Electron pointer fix: a record and
# an override file that holds their own line and the fix's keys. apt removes
# Better Manager, as it is the only way it goes; its prerm runs the
# before-removal program as that person, and its postrm deletes what the
# program set aside. Run under a time limit so a hang fails the job instead
# of holding it.
FIX_UID=1000
fix_user="$(getent passwd "$FIX_UID" | cut -d: -f1 || true)"
if [[ -z "$fix_user" ]]; then
    useradd --create-home --uid "$FIX_UID" better-person
    fix_user=better-person
fi
fix_home="$(getent passwd "$FIX_UID" | cut -d: -f6)"
fix_app=io.github.shiftey.Desktop
fix_override="$fix_home/.local/share/flatpak/overrides/$fix_app"
fix_record="$fix_home/.local/state/better-os/fixes.json"
fix_aside="$fix_home/.local/state/better-os/fixes-removal-undo.json"
own_lines='# kept by hand
[Context]
filesystems=~/Projects;
'
mkdir -p "$(dirname "$fix_override")" "$(dirname "$fix_record")"
printf '%ssockets=x11;!fallback-x11;!wayland;\n\n[Environment]\nELECTRON_OZONE_PLATFORM_HINT=x11\nXDG_SESSION_TYPE=x11\n' \
    "$own_lines" > "$fix_override"
cat > "$fix_record" <<RECORD
{
  "schema_version": 1,
  "applied": [
    {
      "entry": "electron-cursor",
      "subject": { "flatpak-app": "$fix_app" },
      "applied_at": 1763700000,
      "change": {
        "override-keys": {
          "app": "$fix_app",
          "name": { "default": "GitHub Desktop", "zh_tw": null },
          "created_file": false,
          "created_groups": ["Environment"],
          "keys": [
            { "key": { "socket": "x11" }, "previous": "absent", "written": "granted" },
            { "key": { "socket": "fallback-x11" }, "previous": "absent", "written": "denied" },
            { "key": { "socket": "wayland" }, "previous": "absent", "written": "denied" },
            { "key": { "environment": "ELECTRON_OZONE_PLATFORM_HINT" }, "previous": "absent", "written": { "value": "x11" } },
            { "key": { "environment": "XDG_SESSION_TYPE" }, "previous": "absent", "written": { "value": "x11" } }
          ]
        }
      }
    }
  ]
}
RECORD
chown -R "$FIX_UID:$FIX_UID" "$fix_home/.local"
installer_reports_installed better-manager || {
    printf 'Better Manager is not installed before its removal check\n' >&2
    exit 1
}
[[ -x /usr/lib/better-os/before-removal/better-manager ]] || {
    printf 'The before-removal program is not installed\n' >&2
    exit 1
}
timeout -s KILL 300 env DEBIAN_FRONTEND=noninteractive apt-get remove -y better-manager \
    > /tmp/manager-remove.log 2>&1 || {
    printf 'Removing Better Manager failed or did not finish:\n' >&2
    cat /tmp/manager-remove.log >&2
    exit 1
}
if installer_reports_installed better-manager; then
    printf 'Better Manager is still installed after its removal\n' >&2
    exit 1
fi
[[ "$(cat "$fix_override")" == "$(printf '%s' "$own_lines")" ]] || {
    printf 'The override is not back to the person'"'"'s own lines:\n' >&2
    cat "$fix_override" >&2
    cat /tmp/manager-remove.log >&2
    exit 1
}
# The program rewrote the file as the person: a root write would leave it
# owned by root.
[[ "$(stat -c %u "$fix_override")" == "$FIX_UID" ]] || {
    printf 'The override was rewritten by uid %s, not by the person\n' "$(stat -c %u "$fix_override")" >&2
    exit 1
}
[[ ! -e "$fix_record" ]] || {
    printf 'The record outlived Better Manager\n' >&2
    exit 1
}
[[ ! -e "$fix_aside" && ! -e "$fix_aside.tmp" ]] || {
    printf 'What the removal set aside outlived it\n' >&2
    exit 1
}
[[ ! -e /run/better-os/better-manager-removal-run ]] || {
    printf 'The removal stamp outlived the removal\n' >&2
    exit 1
}
if ls "${TMPDIR:-/tmp}"/better-os-accounts.* >/dev/null 2>&1; then
    printf 'A maintainer script left its account library behind\n' >&2
    exit 1
fi
printf 'Removing Better Manager undid the fix, as the person, and left nothing\n'

"$INSTALLER" --uninstall
for package_name in better-manager better-manager-daemon better-shell-adapter; do
    if installer_reports_installed "$package_name"; then
        printf 'The installer did not remove %s\n' "$package_name" >&2
        exit 1
    fi
done
[[ ! -e /usr/bin/better-manager ]] || {
    printf 'The installer left better-manager behind after --uninstall\n' >&2
    exit 1
}
[[ ! -e /usr/libexec/better-manager-daemon ]] || {
    printf 'The installer left the privileged service behind after --uninstall\n' >&2
    exit 1
}
printf 'The installer removed what it installed\n'

printf '== a 0.2.9 service is updated through Better Manager (ADR 0028) ==\n'
# The published 0.2.9 service, whose prerm stops its own unit and which
# refuses companions. CI downloads it into the image; nothing here reaches
# the network. Better Manager's path is the one Better Manager takes for such
# a service: stage the new service and the adapter in its archive, send the
# Better Manager install alone, and let the new package's postinst hand the
# staged packages over.
LEGACY_DAEMON_DEB="$(ls /opt/better-os/legacy/better-manager-daemon_0.2.9_"${RELEASE_TARGET}"_"${ARCH}".deb)"
MANAGER_DEB="$(find_package better-manager)"
ADAPTER_DEB="$(find_package better-shell-adapter)"
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$LEGACY_DAEMON_DEB"
[[ "$(dpkg-query -W -f='${Version}' better-manager-daemon)" == 0.2.9 ]] || {
    printf 'The 0.2.9 service did not install\n' >&2
    exit 1
}
rm -f /run/better-os/daemon-handoff
"$CLIENT" stage "$RELEASE_ID" "$ARCH" "$DAEMON_DEB"
"$CLIENT" stage "$RELEASE_ID" "$ARCH" "$ADAPTER_DEB"
"$CLIENT" install "$RELEASE_ID" "$ARCH" "$MANAGER_DEB" >/dev/null || {
    printf 'The 0.2.9 service did not install Better Manager\n' >&2
    exit 1
}
# The old service leaves once it has been idle for two minutes; the handoff
# waits for that before it installs.
handoff_state=""
for _ in $(seq 300); do
    handoff_state="$(sed -n 1p /run/better-os/daemon-handoff 2>/dev/null || true)"
    [[ "$handoff_state" == done || "$handoff_state" == failed ]] && break
    sleep 2
done
if [[ "$handoff_state" != done ]]; then
    printf 'The handoff ended as %s\n' "${handoff_state:-nothing}" >&2
    cat /run/better-os/daemon-handoff /run/better-os/daemon-handoff.log >&2 2>/dev/null || true
    exit 1
fi
new_version="$(dpkg-deb -f "$DAEMON_DEB" Version)"
[[ "$(dpkg-query -W -f='${Version}' better-manager-daemon)" == "$new_version" ]] || {
    printf 'The service is not at %s after the handoff\n' "$new_version" >&2
    exit 1
}
[[ "$(dpkg-query -W -f='${db:Status-Abbrev}' better-shell-adapter)" == "ii " ]] || {
    printf 'The shell adapter is not installed after the handoff\n' >&2
    exit 1
}
audit="$(dpkg --audit)"
[[ -z "$audit" ]] || {
    printf 'dpkg is not consistent after the handoff:\n%s\n' "$audit" >&2
    exit 1
}
printf 'The 0.2.9 service was replaced from outside it, with the adapter\n'
DEBIAN_FRONTEND=noninteractive apt-get remove -y better-manager better-shell-adapter

printf '== purge leaves nothing behind ==\n'
DEBIAN_FRONTEND=noninteractive apt-get purge -y better-manager-daemon
for leftover in /var/lib/better-os /var/cache/better-os; do
    [[ ! -e "$leftover" ]] || {
        printf 'Purge left %s behind\n' "$leftover" >&2
        exit 1
    }
done

printf 'Daemon end-to-end check passed on %s\n' "$ARCH"
