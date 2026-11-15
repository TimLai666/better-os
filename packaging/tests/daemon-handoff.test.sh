#!/usr/bin/env bash
# shellcheck disable=SC2030,SC2031 # each case exports its own fakes' answers in a subshell
# Drives packaging/maintainer/daemon-handoff.sh (ADR 0028, ticket 124) against
# fake dpkg-query, dpkg-deb, systemctl, systemd-run, pidof, and apt-get, with
# a scratch archive and status directory. Nothing here reaches a real package
# manager or systemd: the fakes are first on PATH and only write to a log.
# `dpkg --compare-versions` is the real one; it reads nothing.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
HANDOFF="$ROOT_DIR/packaging/maintainer/daemon-handoff.sh"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf -- "$WORK_DIR"' EXIT

FAILURES=0
# Each case runs in a subshell; a failure makes the subshell fail, which the
# caller counts.
fail() {
    printf 'FAIL: %s\n' "$1" >&2
    return 1
}
pass() {
    printf 'ok: %s\n' "$1"
}

FAKE_BIN="$WORK_DIR/bin"
mkdir -p "$FAKE_BIN"
LOG="$WORK_DIR/calls.log"
export LOG

# dpkg-query -W -f='${Version}' PACKAGE: answers from $FAKE_INSTALLED.
# shellcheck disable=SC2016 # the fakes are written verbatim
cat > "$FAKE_BIN/dpkg-query" <<'FAKE'
#!/bin/sh
[ -n "${FAKE_INSTALLED:-}" ] || exit 1
printf '%s' "$FAKE_INSTALLED"
FAKE
# dpkg-deb -f FILE Version: the version is the second field of the file name.
cat > "$FAKE_BIN/dpkg-deb" <<'FAKE'
#!/bin/sh
name="${2##*/}"
name="${name#*_}"
printf '%s\n' "${name%%_*}"
FAKE
# systemctl is-active --quiet UNIT: active for the first $FAKE_ACTIVE_CHECKS.
cat > "$FAKE_BIN/systemctl" <<'FAKE'
#!/bin/sh
count_file="${LOG%/*}/active.count"
count="$(cat "$count_file" 2>/dev/null || echo 0)"
count=$((count + 1))
echo "$count" > "$count_file"
printf 'systemctl %s\n' "$*" >> "$LOG"
[ "$count" -le "${FAKE_ACTIVE_CHECKS:-0}" ]
FAKE
cat > "$FAKE_BIN/systemd-run" <<'FAKE'
#!/bin/sh
printf 'systemd-run %s\n' "$*" >> "$LOG"
FAKE
cat > "$FAKE_BIN/pidof" <<'FAKE'
#!/bin/sh
exit 1
FAKE
cat > "$FAKE_BIN/apt-get" <<'FAKE'
#!/bin/sh
printf 'apt-get %s\n' "$*" >> "$LOG"
exit "${FAKE_APT_EXIT:-0}"
FAKE
chmod 0755 "$FAKE_BIN"/*

# A fresh scratch machine: an archive holding the staged packages, the
# checksum list this release was built with, and a systemd directory.
setup() {
    local case_dir="$WORK_DIR/$1"
    mkdir -p "$case_dir/archive" "$case_dir/status" "$case_dir/systemd" "$case_dir/lib"
    printf 'service bytes\n' > "$case_dir/archive/better-manager-daemon_0.2.16_ubuntu-24.04_amd64.deb"
    printf 'adapter bytes\n' > "$case_dir/archive/better-shell-adapter_0.2.16_ubuntu-24.04_amd64.deb"
    (
        cd "$case_dir/archive"
        sha256sum better-manager-daemon_0.2.16_ubuntu-24.04_amd64.deb \
            better-shell-adapter_0.2.16_ubuntu-24.04_amd64.deb
    ) > "$case_dir/lib/packages"
    : > "$LOG"
    rm -f "$WORK_DIR/active.count"
    export BETTER_OS_HANDOFF_EXPECTED="$case_dir/lib/packages"
    export BETTER_OS_HANDOFF_ARCHIVE="$case_dir/archive"
    export BETTER_OS_HANDOFF_STATUS_DIR="$case_dir/status"
    export BETTER_OS_HANDOFF_SYSTEMD_DIR="$case_dir/systemd"
    export BETTER_OS_HANDOFF_SELF="$HANDOFF"
    export BETTER_OS_HANDOFF_IDLE_INTERVAL=0
    export BETTER_OS_HANDOFF_IDLE_CHECKS=5
    export PATH="$FAKE_BIN:$PATH"
    CASE_DIR="$case_dir"
}

state() {
    sed -n 1p "$CASE_DIR/status/daemon-handoff" 2>/dev/null || true
}

# A 0.2.9 service and a verified staged package: the postinst starts the
# handoff in its own transient unit.
(
    setup start-legacy
    export FAKE_INSTALLED=0.2.9
    sh "$HANDOFF" start
    if grep -q "^systemd-run .*--unit=better-manager-daemon-handoff .*-- $HANDOFF run$" "$LOG" &&
        [[ "$(state)" == waiting ]]; then
        pass "a legacy service is handed to a unit of its own"
    else
        fail "the handoff did not start: $(cat "$LOG") / $(state)"
    fi
) || FAILURES=$((FAILURES + 1))

# 0.2.13 and later take their own update from Better Manager: no handoff.
(
    setup start-safe
    export FAKE_INSTALLED=0.2.13
    sh "$HANDOFF" start
    if [[ ! -s "$LOG" && -z "$(state)" ]]; then
        pass "a service that updates itself gets no handoff"
    else
        fail "a 0.2.13 service was handed off: $(cat "$LOG")"
    fi
) || FAILURES=$((FAILURES + 1))

# A staged file whose checksum is not the build's is never handed over.
(
    setup start-tampered
    export FAKE_INSTALLED=0.2.9
    printf 'other bytes\n' > "$CASE_DIR/archive/better-manager-daemon_0.2.16_ubuntu-24.04_amd64.deb"
    sh "$HANDOFF" start
    if [[ ! -s "$LOG" ]]; then
        pass "a package with another checksum is not handed over"
    else
        fail "a tampered package started the handoff: $(cat "$LOG")"
    fi
) || FAILURES=$((FAILURES + 1))

# The run waits for the old service to go idle, then installs the service and
# the adapter in one apt-get run.
(
    setup run-ok
    export FAKE_INSTALLED=0.2.9 FAKE_ACTIVE_CHECKS=2
    sh "$HANDOFF" run
    active="$(grep -c '^systemctl is-active' "$LOG")"
    if [[ "$active" == 3 ]] &&
        grep -q '^apt-get install .*-- .*better-manager-daemon_0.2.16.* .*better-shell-adapter_0.2.16' "$LOG" &&
        [[ "$(state)" == done ]]; then
        pass "the run waits for the old service, then installs both packages"
    else
        fail "the run did not wait and install: $(cat "$LOG") / $(state)"
    fi
) || FAILURES=$((FAILURES + 1))

# A service that never goes idle is a recorded failure, and nothing is
# installed under it.
(
    setup run-busy
    export FAKE_INSTALLED=0.2.9 FAKE_ACTIVE_CHECKS=100
    if sh "$HANDOFF" run; then
        fail "a busy service let the run succeed"
    elif ! grep -q '^apt-get' "$LOG" && [[ "$(state)" == failed ]]; then
        pass "a service that stays busy fails the handoff without installing"
    else
        fail "a busy service: $(cat "$LOG") / $(state)"
    fi
) || FAILURES=$((FAILURES + 1))

# apt failing is a recorded failure too.
(
    setup run-apt-fails
    export FAKE_INSTALLED=0.2.9 FAKE_APT_EXIT=100
    if sh "$HANDOFF" run; then
        fail "a failed apt-get let the run succeed"
    elif [[ "$(state)" == failed ]] && grep -q 'apt-get could not install' "$CASE_DIR/status/daemon-handoff"; then
        pass "an apt failure is written down"
    else
        fail "an apt failure: $(state)"
    fi
) || FAILURES=$((FAILURES + 1))

# Without systemd there is no unit to be stopped from inside: the run is
# detached and finishes on its own.
(
    setup start-no-systemd
    export FAKE_INSTALLED=0.2.9
    rmdir "$CASE_DIR/systemd"
    sh "$HANDOFF" start
    for _ in $(seq 50); do
        [[ "$(state)" == done ]] && break
        sleep 0.1
    done
    if ! grep -q '^systemd-run' "$LOG" && [[ "$(state)" == done ]]; then
        pass "without systemd the handoff runs detached"
    else
        fail "without systemd: $(cat "$LOG") / $(state)"
    fi
) || FAILURES=$((FAILURES + 1))

if ((FAILURES > 0)); then
    printf '%d handoff check(s) failed\n' "$FAILURES" >&2
    exit 1
fi
