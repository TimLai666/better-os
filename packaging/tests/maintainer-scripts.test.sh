#!/usr/bin/env bash
# Drives packaging/maintainer/user-units.sh, the user-unit half of every
# Better OS package's preinst and postinst (ticket 105), against a fake
# systemctl, systemd-run, and timeout, and scratch copies of /run/user and
# the unit directory. Nothing here reaches a real systemd manager: the fakes
# are first on PATH and only write to a log.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
LIBRARY="$ROOT_DIR/packaging/maintainer/user-units.sh"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf -- "$WORK_DIR"' EXIT

FAILURES=0

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    FAILURES=$((FAILURES + 1))
}

pass() {
    printf 'ok: %s\n' "$1"
}

FAKE_BIN="$WORK_DIR/bin"
LOG="$WORK_DIR/systemctl.log"
mkdir -p "$FAKE_BIN"


# systemd-run [--flags] [-p NAME=VALUE]... [-E NAME=VALUE]... COMMAND...:
# logs how it was asked, checks a per-user run names the user and a runtime
# limit and opens no PAM session, sets the -E variables, and runs COMMAND. It
# keeps PATH as it is, so whatever it runs is still only a fake.
cat > "$FAKE_BIN/systemd-run" <<'FAKE'
#!/bin/sh
printf 'systemd-run %s\n' "$*" >> "${0%/bin/systemd-run}/systemd-run.log"
user=""
limit=""
while [ $# -gt 0 ]; do
    case "$1" in
        -p)
            case "$2" in
                User=*) user="${2#User=}" ;;
                RuntimeMaxSec=*) limit="${2#RuntimeMaxSec=}" ;;
                PAMName=*) exit 8 ;;
            esac
            shift 2
            ;;
        -E)
            export "$2"
            shift 2
            ;;
        --*) shift ;;
        *) break ;;
    esac
done
if [ "$1" = systemctl ]; then
    [ -n "$user" ] && [ -n "$limit" ] || exit 9
    [ "${XDG_RUNTIME_DIR##*/}" = "$user" ] || exit 9
fi
exec "$@"
FAKE

# systemctl --user VERB [FLAGS] UNIT: logs "uid verb unit" and answers
# is-enabled from <root>/state/<uid>/enabled. Whose manager it is comes from
# the runtime directory systemd-run was told to give it.
cat > "$FAKE_BIN/systemctl" <<'FAKE'
#!/bin/sh
root="${XDG_RUNTIME_DIR%/run/user/*}"
FAKE_UID="${XDG_RUNTIME_DIR##*/}"
PATH=/usr/bin:/bin
log="$root/systemctl.log"
[ "$1" = --user ] || { echo "not --user: $*" >> "$log"; exit 3; }
shift
state="$root/state/$FAKE_UID"
verb="$1"
shift
unit=""
for argument in "$@"; do
    case "$argument" in
        --*) ;;
        *) unit="$argument" ;;
    esac
done
printf '%s %s %s\n' "$FAKE_UID" "$verb" "$unit" >> "$log"
case "$verb" in
    is-enabled) grep -qxF "$unit" "$state/enabled" 2>/dev/null ;;
    daemon-reload | try-restart | enable) exit 0 ;;
    *) exit 4 ;;
esac
FAKE
chmod +x "$FAKE_BIN"/*

# A user manager's private socket. dpkg's postinst checks for a socket, so a
# plain file would not stand in for one.
make_socket() {
    python3 -c 'import socket, sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' "$1"
}

reset_world() {
    rm -rf "$WORK_DIR/root" "$LOG"
    mkdir -p "$WORK_DIR/root/run/user" "$WORK_DIR/root/units" "$WORK_DIR/root/state-run"
    : > "$LOG"
}

# A person logged in, with their manager running and these units enabled.
logged_in() {
    local uid="$1"
    shift
    mkdir -p "$WORK_DIR/root/run/user/$uid/systemd" "$WORK_DIR/root/state/$uid"
    make_socket "$WORK_DIR/root/run/user/$uid/systemd/private"
    : > "$WORK_DIR/root/state/$uid/enabled"
    local unit
    for unit in "$@"; do
        printf '%s\n' "$unit" >> "$WORK_DIR/root/state/$uid/enabled"
    done
}

# Runs one of the library's entry points the way the generated script does.
run_script() {
    local units="$1" entry="$2"
    shift 2
    (
        export PATH="$FAKE_BIN:$PATH"
        # The fakes find their state next to the scratch /run/user.
        better_os_package='better-awake'
        better_os_user_units="$units"
        better_os_unit_dir="$WORK_DIR/root/units"
        better_os_run_user_root="$WORK_DIR/root/run/user"
        better_os_state_dir="$WORK_DIR/root/state-run"
        # Only the fakes: the real systemctl is never on this PATH.
        better_os_path="$FAKE_BIN"
        better_os_cgroup_file="$WORK_DIR/root/cgroup"
        # shellcheck source=packaging/maintainer/user-units.sh
        . "$LIBRARY"
        "$entry" "$@"
    )
    # The fake systemctl writes into the scratch root; gather it.
    if [[ -f "$WORK_DIR/root/systemctl.log" ]]; then
        cat "$WORK_DIR/root/systemctl.log" >> "$LOG"
        rm -f "$WORK_DIR/root/systemctl.log"
    fi
}

expect_log() {
    local line="$1" label="$2"
    if grep -qxF "$line" "$LOG"; then
        pass "$label"
    else
        fail "$label: no \"$line\" in: $(tr '\n' ';' < "$LOG")"
    fi
}

expect_no_log() {
    local pattern="$1" label="$2"
    if grep -qE "$pattern" "$LOG"; then
        fail "$label: found /$pattern/ in: $(tr '\n' ';' < "$LOG")"
    else
        pass "$label"
    fi
}

# --- a first install leaves every unit to Better Manager -------------------
reset_world
logged_in 1000 better-awake.service
run_script 'better-awake.service' better_os_user_units_after_upgrade ''
if [[ ! -s "$LOG" ]]; then
    pass "a first install touches no user manager"
else
    fail "a first install touched a user manager: $(cat "$LOG")"
fi

# --- an upgrade restarts what runs, for everyone logged in -----------------
reset_world
touch "$WORK_DIR/root/units/better-awake.service"
logged_in 1000 better-awake.service
logged_in 1001
run_script 'better-awake.service' better_os_user_units_before_upgrade upgrade
run_script 'better-awake.service' better_os_user_units_after_upgrade 0.2.12
expect_log "1000 daemon-reload " "the person's manager rereads the new unit file"
expect_log "1000 try-restart better-awake.service" "a running unit is restarted"
expect_log "1001 daemon-reload " "every logged-in person's manager is reloaded"
expect_log "1001 try-restart better-awake.service" \
    "a person who turned the unit off gets try-restart, which starts nothing"
expect_no_log '^[0-9]+ (start|restart|enable|reenable) ' \
    "nothing is started or enabled when no unit is new"
if [[ ! -e "$WORK_DIR/root/state-run/better-awake.units" ]]; then
    pass "the upgrade record is removed afterwards"
else
    fail "the upgrade record was left behind"
fi

# --- a unit new in this version follows the component ----------------------
reset_world
touch "$WORK_DIR/root/units/better-awake.service"
logged_in 1000 better-awake.service
logged_in 1001
run_script 'better-awake.service better-awake-tray.service' \
    better_os_user_units_before_upgrade upgrade
# dpkg unpacks the new unit file between the two scripts.
touch "$WORK_DIR/root/units/better-awake-tray.service"
run_script 'better-awake.service better-awake-tray.service' \
    better_os_user_units_after_upgrade 0.2.12
expect_log "1000 enable better-awake-tray.service" \
    "the new tray unit is enabled and started for a person with the component on"
expect_log "1000 try-restart better-awake.service" "the old unit is still restarted"
expect_no_log '^1001 enable ' "a person with the component off gets no new unit"
expect_no_log ' try-restart better-awake-tray.service$' \
    "the new unit is started by enable --now, not restarted"

# --- with no record of what came before, nothing counts as new -------------
reset_world
logged_in 1000 better-awake.service
run_script 'better-awake.service better-awake-tray.service' \
    better_os_user_units_after_upgrade 0.2.12
expect_no_log ' enable ' "without the preinst's record no unit is enabled"
expect_log "1000 try-restart better-awake-tray.service" \
    "every unit is treated as old and only restarted if it runs"

# --- only real user managers are asked ----------------------------------------
reset_world
logged_in 1000 better-awake.service
mkdir -p "$WORK_DIR/root/run/user/0/systemd" "$WORK_DIR/root/run/user/notauid/systemd"
make_socket "$WORK_DIR/root/run/user/0/systemd/private"
make_socket "$WORK_DIR/root/run/user/notauid/systemd/private"
mkdir -p "$WORK_DIR/root/run/user/1002"
run_script 'better-awake.service' better_os_user_units_after_upgrade 0.2.12
expect_no_log '^0 ' "root's runtime directory is not a person's session"
expect_no_log '^notauid ' "a directory that is not a uid is skipped"
expect_no_log '^1002 ' "a runtime directory with no manager socket is skipped"
expect_log "1000 try-restart better-awake.service" "the person logged in still is"

# --- a failing manager does not fail the package ---------------------------
reset_world
logged_in 1000 better-awake.service
cat > "$FAKE_BIN/systemctl.broken" <<'FAKE'
#!/bin/sh
exit 1
FAKE
chmod +x "$FAKE_BIN/systemctl.broken"
mv "$FAKE_BIN/systemctl" "$FAKE_BIN/systemctl.good"
mv "$FAKE_BIN/systemctl.broken" "$FAKE_BIN/systemctl"
if run_script 'better-awake.service' better_os_user_units_after_upgrade 0.2.12; then
    pass "a manager that refuses everything does not stop the upgrade"
else
    fail "an unreachable user manager failed the postinst"
fi
mv "$FAKE_BIN/systemctl.good" "$FAKE_BIN/systemctl"

# --- the preinst only records on an upgrade ----------------------------------
reset_world
touch "$WORK_DIR/root/units/better-awake.service"
run_script 'better-awake.service' better_os_user_units_before_upgrade install
if [[ ! -e "$WORK_DIR/root/state-run/better-awake.units" ]]; then
    pass "a first install records nothing"
else
    fail "a first install wrote an upgrade record"
fi

# --- inside Better Manager's service the restart runs outside its sandbox --
# The postinst is built the way build-deb.sh builds it and run as dpkg would.
make_postinst() {
    {
        printf '#!/bin/sh\nset -e\n'
        printf "better_os_package='better-awake'\n"
        printf "better_os_user_units='better-awake.service'\n"
        printf "better_os_unit_dir='%s'\n" "$WORK_DIR/root/units"
        printf "better_os_run_user_root='%s'\n" "$WORK_DIR/root/run/user"
        printf "better_os_state_dir='%s'\n" "$WORK_DIR/root/state-run"
        printf "better_os_path='%s'\n" "$FAKE_BIN"
        printf "better_os_cgroup_file='%s'\n" "$WORK_DIR/root/cgroup"
        cat "$LIBRARY"
        # shellcheck disable=SC2016 # expanded by the generated script
        printf 'better_os_user_units_postinst "$@" || true\n'
    } > "$WORK_DIR/root/postinst"
    chmod +x "$WORK_DIR/root/postinst"
}
export SYSTEMD_RUN_LOG="$WORK_DIR/systemd-run.log"
: > "$SYSTEMD_RUN_LOG"

reset_world
: > "$SYSTEMD_RUN_LOG"
logged_in 1000 better-awake.service
printf '0::/system.slice/better-manager-daemon.service\n' > "$WORK_DIR/root/cgroup"
make_postinst
PATH="$FAKE_BIN:$PATH" "$WORK_DIR/root/postinst" configure 0.2.12
cat "$WORK_DIR/root/systemctl.log" >> "$LOG" 2>/dev/null || true
if grep -q "^systemd-run .*$WORK_DIR/root/postinst better-os-user-units 0.2.12\$" "$SYSTEMD_RUN_LOG"; then
    pass "inside the daemon's unit the postinst runs itself again outside it"
else
    fail "the postinst did not leave the daemon's sandbox: $(cat "$SYSTEMD_RUN_LOG")"
fi
expect_log "1000 try-restart better-awake.service" "and the person's unit is restarted from there"

reset_world
: > "$SYSTEMD_RUN_LOG"
logged_in 1000 better-awake.service
printf '0::/user.slice/user-1000.slice/session-2.scope\n' > "$WORK_DIR/root/cgroup"
make_postinst
PATH="$FAKE_BIN:$PATH" "$WORK_DIR/root/postinst" configure 0.2.12
cat "$WORK_DIR/root/systemctl.log" >> "$LOG" 2>/dev/null || true
if ! grep -q "better-os-user-units" "$SYSTEMD_RUN_LOG"; then
    pass "an apt run from a terminal restarts the units directly"
else
    fail "the postinst ran itself again outside the daemon: $(cat "$SYSTEMD_RUN_LOG")"
fi
expect_log "1000 try-restart better-awake.service" "the unit is restarted"

# --- a person's manager is reached with no login session ---------------------
reset_world
: > "$SYSTEMD_RUN_LOG"
logged_in 1000 better-awake.service
run_script 'better-awake.service' better_os_user_units_after_upgrade 0.2.12
if grep -q -- "-p User=1000 -p RuntimeMaxSec=30 -E XDG_RUNTIME_DIR=$WORK_DIR/root/run/user/1000 systemctl --user try-restart" "$SYSTEMD_RUN_LOG" \
    && ! grep -q "PAMName" "$SYSTEMD_RUN_LOG"; then
    pass "systemctl runs as the person, from systemd, with a deadline and no PAM session"
else
    fail "unexpected per-user run: $(cat "$SYSTEMD_RUN_LOG")"
fi

# --- the daemon's postinst only holds a restart while the daemon is busy ------
DAEMON_LIBRARY="$ROOT_DIR/packaging/maintainer/daemon.sh"
in_progress() {
    (
        better_os_daemon_busy="$WORK_DIR/run/better-os/daemon-busy"
        # shellcheck source=packaging/maintainer/daemon.sh
        . "$DAEMON_LIBRARY"
        better_os_daemon_transaction_in_progress
    )
}
rm -rf "$WORK_DIR/run"
if ! in_progress; then
    pass "no marker: the daemon is idle or not running, and may be restarted"
else
    fail "a restart was held with no marker"
fi
mkdir -p "$WORK_DIR/run/better-os"
printf '1234\n' > "$WORK_DIR/run/better-os/daemon-busy"
if in_progress; then
    pass "the marker the daemon writes while a transaction runs holds the restart"
else
    fail "a running transaction did not hold the restart"
fi
rm -f "$WORK_DIR/run/better-os/daemon-busy"

# --- the file is POSIX sh, as dpkg runs it ------------------------------------
if sh -n "$LIBRARY" && sh -n "$DAEMON_LIBRARY"; then
    pass "the library parses as sh"
else
    fail "the library does not parse as sh"
fi

if ((FAILURES > 0)); then
    printf '%d check(s) failed\n' "$FAILURES" >&2
    exit 1
fi
printf 'all maintainer script checks passed\n'
