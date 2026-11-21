#!/usr/bin/env bash
# The Better OS login supervisor and its backstop against a real systemd,
# logind and D-Bus (design/login-screen-spec.md section 5.10, ticket 267):
#
#   A. a stand-in greeter that never becomes ready: three attempts, then the
#      text login on tty1 within 80 s, the journal lines, the record, the
#      message, the bus interface and the status command; the supervisor
#      untouched by every stop, start and restart of the greeter's unit;
#   B. the supervisor killed in the middle of its second attempt: it
#      continues from /run/better-os/login-path.json with the same T0;
#   C. a supervisor that cannot start: after five starts the backstop gives
#      the text login on tty2 within 90 s, and does nothing when the
#      greeter was ready or a fall back runs.
#
# Runs only inside the container packaging/boot-watch/run.sh starts: it
# covers the terminals and /sys/class/drm with stand-ins and starts a
# display manager.
set -euo pipefail
[[ "${BETTER_OS_E2E_CONTAINER:-}" == 1 ]] || {
    echo "login-path.sh starts a display manager; run it through packaging/boot-watch/run.sh" >&2
    exit 2
}

SUPERVISOR=better-os-login-supervisor.service
GREETER=better-os-greeter.service
BACKSTOP=better-os-login-backstop.service
RUN=/run/better-os
RECORDS=/var/lib/better-os/boot-watch

dump() {
    echo "--- journal of the login path ---" >&2
    journalctl -b --no-pager -o short-monotonic \
        -u "$SUPERVISOR" -u "$GREETER" -u "$BACKSTOP" -u better-os-login-backstop.timer \
        -u 'better-os-login-fallback@*' 2>/dev/null | tail -n 120 >&2 || true
    echo "--- $RUN ---" >&2
    ls -la "$RUN" >&2 2>/dev/null || true
    cat "$RUN/login-path.json" >&2 2>/dev/null || true
}
fail() { echo "FAIL: $*" >&2; dump; exit 1; }
pass() { echo "ok: $*"; }
# expect <what> <expected> <found>
expect() { [[ "$2" == "$3" ]] || fail "$1: expected '$2', found '$3'"; }
show() { systemctl show -p "$2" --value "$1"; }
# wait_for <what> <seconds> <command...>
wait_for() {
    local what="$1" limit="$2"
    shift 2
    local i
    for ((i = 0; i < limit * 4; i++)); do
        "$@" && return 0
        sleep 0.25
    done
    fail "$what: expected within ${limit} s, not seen"
}
# The journal of one unit since the last `mark`.
cursor=
mark() { cursor="$(journalctl -b -n 0 --show-cursor --no-pager | sed -n 's/^-- cursor: //p')"; }
journal() {
    if [[ -n "$cursor" ]]; then
        journalctl -b --no-pager -o cat --after-cursor="$cursor" -u "$1" 2>/dev/null
    else
        journalctl -b --no-pager -o cat -u "$1" 2>/dev/null
    fi
}
retried() { journal "$SUPERVISOR" | grep -q 'RESULT=retried'; }
seconds_between() { echo $((($2 - $1) / 1000000)); }

# --- The stand-ins -----------------------------------------------------------

# A privileged container sees the host's terminals: every terminal the
# supervisor or chvt opens here is /dev/null, so nothing reaches them.
for tty in tty0 tty1 tty2; do
    [[ -e "/dev/$tty" ]] || : > "/dev/$tty"
    mount --bind /dev/null "/dev/$tty"
done

# One virtio card with a connected screen, so stage 1 passes at once.
fake=/run/fake-drm
mkdir -p "$fake/card0/device" "$fake/card0-Virtual-1"
ln -s ../../../bus/pci/drivers/virtio-pci "$fake/card0/device/driver"
echo connected > "$fake/card0-Virtual-1/status"
if [[ ! -d /sys/class/drm ]]; then
    # The host has no DRM at all: a directory cannot be made in sysfs, so
    # the class directory is covered, the other classes with it.
    echo "note: the host has no /sys/class/drm; covering /sys/class" >&2
    mount -t tmpfs tmpfs /sys/class
    mkdir /sys/class/drm
fi
mount --bind "$fake" /sys/class/drm

# Plymouth: every command it is given, written where the supervisor's
# sandbox lets it write.
# shellcheck disable=SC2016 # "$*" belongs to the stand-in, not to this script
printf '#!/bin/sh\necho "$*" >> %s/plymouth-calls\n' "$RUN" > /usr/bin/plymouth
chmod 755 /usr/bin/plymouth

install -m 0755 /login/better-os-login-supervisor /usr/sbin/better-os-login-supervisor
install -m 0755 /login/better-os-login-status /usr/bin/better-os-login-status
for unit in "$SUPERVISOR" "$GREETER" "$BACKSTOP" better-os-login-backstop.timer \
    better-os-login-fallback@.service; do
    install -m 0644 "/login/greeter/$unit" "/usr/lib/systemd/system/$unit"
done
install -m 0644 /login/greeter/org.betteros.LoginSupervisor1.conf \
    /usr/share/dbus-1/system.d/org.betteros.LoginSupervisor1.conf
install -D -m 0644 /login/greeter/login-supervisor-failed.issue \
    /usr/share/better-os/login-supervisor-failed.issue

# The greeter never becomes ready: no greetd, no greeter session, no
# heartbeat.
mkdir -p "/etc/systemd/system/$GREETER.d"
printf '[Service]\nExecStart=\nExecStart=/bin/sleep infinity\n' \
    > "/etc/systemd/system/$GREETER.d/stand-in.conf"
# The text login, without a terminal to run on: it only has to be started.
mkdir -p /etc/systemd/system/better-os-login-fallback@.service.d
printf '%s\n' '[Service]' 'ExecStart=' 'ExecStart=/bin/sleep infinity' \
    'StandardInput=null' 'StandardOutput=journal' 'TTYReset=no' 'TTYVHangup=no' \
    'TTYVTDisallocate=no' 'UtmpIdentifier=' \
    > /etc/systemd/system/better-os-login-fallback@.service.d/stand-in.conf

# The display manager is the supervisor, as the package's debconf
# registration makes it.
ln -sf "/usr/lib/systemd/system/$SUPERVISOR" /etc/systemd/system/display-manager.service
systemctl daemon-reload
systemctl reload dbus.service
systemctl start systemd-logind.service
expect "logind" active "$(show systemd-logind.service ActiveState)"
expect "display-manager.service" "$SUPERVISOR" "$(show display-manager.service Id)"
verify="$(systemd-analyze verify "/usr/lib/systemd/system/$SUPERVISOR" \
    "/usr/lib/systemd/system/$BACKSTOP" /usr/lib/systemd/system/better-os-login-backstop.timer 2>&1)" \
    || fail "systemd-analyze verify: expected the units accepted, found: $verify"

reset() {
    systemctl stop display-manager.service "$GREETER" "$BACKSTOP" better-os-login-backstop.timer \
        better-os-login-fallback@tty1.service better-os-login-fallback@tty2.service 2>/dev/null || true
    systemctl reset-failed 2>/dev/null || true
    rm -f "$RUN/login-path.json" "$RUN/login-fallback.issue" "$RUN/login-fallback.active" \
        "$RUN/login-held" "$RUN/greeter-mode" "$RUN/plymouth-calls"
    rm -rf /run/better-os-greeter "$RECORDS/evidence" "$RECORDS/summary.json"
}
greeter_starts() { journal "$GREETER" | grep -c '^Started ' || true; }
mark

# --- A. The greeter never becomes ready -------------------------------------

reset
mark
started=$SECONDS
systemctl start display-manager.service
t0="$(show "$SUPERVISOR" InactiveExitTimestampMonotonic)"
invocation="$(show "$SUPERVISOR" InvocationID)"
wait_for "the text login on tty1" 85 systemctl is-active --quiet better-os-login-fallback@tty1.service
text_login="$(show better-os-login-fallback@tty1.service ActiveEnterTimestampMonotonic)"
after="$(seconds_between "$t0" "$text_login")"
((after + 3 <= 80)) || fail "the text login: expected to accept input within 80 s of T0, found $((after + 3)) s"
pass "the greeter never ready: the text login on tty1 ${after} s after T0 ($((SECONDS - started)) s here)"

supervisor_journal="$(journal "$SUPERVISOR")"
for line in "BETTER_OS_LOGIN_STAGE=5 RESULT=retried REASON=no-greeter-session" \
    "BETTER_OS_LOGIN_STAGE=5 RESULT=fell-back REASON=no-greeter-session"; do
    grep -qxF "$line" <<<"$supervisor_journal" || fail "the journal: expected the line '$line'"
done
expect "the greeter's starts" 3 "$(greeter_starts)"
expect "the third attempt's mode" safe "$(cat "$RUN/greeter-mode" 2>/dev/null)"
expect "the supervisor's restarts" 0 "$(show "$SUPERVISOR" NRestarts)"
expect "the supervisor after the fall back" active "$(show "$SUPERVISOR" ActiveState)"
expect "the fall back marker" tty1 "$(cat "$RUN/login-fallback.active" 2>/dev/null)"
[[ -e "$RUN/login-held" ]] || fail "login-held: expected after the fall back, not found"
grep -qx deactivate "$RUN/plymouth-calls" || fail "plymouth: expected 'deactivate', found $(tr '\n' ' ' < "$RUN/plymouth-calls")"
grep -qx quit "$RUN/plymouth-calls" || fail "plymouth: expected 'quit', found $(tr '\n' ' ' < "$RUN/plymouth-calls")"
grep -qF "couldn't be shown (the sign-in service didn't start, " "$RUN/login-fallback.issue" \
    || fail "the message: expected stage 5's words, found: $(head -n1 "$RUN/login-fallback.issue")"
expect "the state file's mode" 644 "$(stat -c %a "$RUN/login-path.json")"
pass "journal lines, three attempts (the third in safe mode), Plymouth, the message"

shopt -s nullglob
events=("$RECORDS"/evidence/[0-9]*-*.json)
expect "the event records" 1 "${#events[@]}"
grep -q '"result": "text_login_used"' "${events[0]}" || fail "the record: expected text_login_used, found $(cat "${events[0]}")"
grep -q '"stage": "sign_in_service"' "${events[0]}" || fail "the record: expected stage sign_in_service"
grep -q '"display_manager": "better-os-login-supervisor.service"' "${events[0]}" || fail "the record names another display manager"
expect "the record's owner and mode" 640:root:adm "$(stat -c %a:%U:%G "${events[0]}")"
expect "the summary's mode" 644 "$(stat -c %a "$RECORDS/summary.json")"
grep -q text_login_used "$RECORDS/summary.json" || fail "the summary: expected the event"
pass "the record, in ADR 0033's place and format"

reply="$(setpriv --reuid=nobody --regid=nogroup --clear-groups \
    busctl call org.betteros.LoginSupervisor1 /org/betteros/LoginSupervisor1 \
    org.betteros.LoginSupervisor1 LastRecord 2>&1)" || fail "LastRecord as nobody: $reply"
grep -q 'fell_back' <<<"$reply" || fail "LastRecord: expected the fall back, found $reply"
if setpriv --reuid=nobody --regid=nogroup --clear-groups busctl call org.betteros.LoginSupervisor1 \
    /org/betteros/LoginSupervisor1 org.freedesktop.DBus.Properties GetAll s \
    org.betteros.LoginSupervisor1 >/dev/null 2>&1; then
    fail "the bus policy: expected only LastRecord open to nobody, found Properties open too"
fi
status="$(TERM=linux LANG=zh_TW.UTF-8 setpriv --reuid=nobody --regid=nogroup --clear-groups \
    better-os-login-status)" || fail "better-os-login-status: $status"
grep -qxF "Now: using the text sign-in (tty1)" <<<"$status" \
    || fail "better-os-login-status on the console: expected English and the fall back, found: $status"
grep -q "read from its files" <<<"$status" && fail "better-os-login-status did not reach the supervisor: $status"
pass "org.betteros.LoginSupervisor1 answers anyone, and the status command prints English on the console"

for step in "start $GREETER" "restart $GREETER" "stop $GREETER" \
    "start better-os-login-fallback@tty2.service" "stop better-os-login-fallback@tty2.service"; do
    # shellcheck disable=SC2086
    systemctl $step
    expect "the supervisor after 'systemctl $step'" "$invocation" "$(show "$SUPERVISOR" InvocationID)"
done
expect "the supervisor after the greeter's unit was stopped and restarted" active "$(show "$SUPERVISOR" ActiveState)"
pass "starting, restarting and stopping the greeter's unit never reach the supervisor"

# --- B. Killed in the middle of the second attempt -------------------------

reset
mark
systemctl start display-manager.service
t0="$(show "$SUPERVISOR" InactiveExitTimestampMonotonic)"
wait_for "the first retry" 40 retried
sleep 2
systemctl kill -s KILL "$SUPERVISOR"
wait_for "the supervisor's restart" 10 test "$(show "$SUPERVISOR" NRestarts)" -ge 1
wait_for "the text login on tty1" 85 systemctl is-active --quiet better-os-login-fallback@tty1.service
text_login="$(show better-os-login-fallback@tty1.service ActiveEnterTimestampMonotonic)"
after="$(seconds_between "$t0" "$text_login")"
((after + 3 <= 80)) || fail "after a restart: expected the text login within 80 s of the first start, found $((after + 3)) s"
expect "T0 kept across the restart" "$t0" \
    "$(grep -o '"t0":[0-9]*' "$RUN/login-path.json" | cut -d: -f2)"
journal "$SUPERVISOR" | grep -qxF "BETTER_OS_LOGIN_STAGE=supervisor RESULT=ok REASON=restarted" \
    || fail "the journal: expected the restart's line"
expect "the greeter's starts across the restart" 3 "$(greeter_starts)"
pass "killed mid-attempt: restarted, continued from its state file, the text login ${after} s after the first T0"

# --- C. A supervisor that cannot start ----------------------------------------

reset
mkdir -p "/etc/systemd/system/$SUPERVISOR.d"
printf '[Service]\nExecStart=\nExecStart=/bin/false\n' > "/etc/systemd/system/$SUPERVISOR.d/broken.conf"
systemctl daemon-reload
started=$SECONDS
systemctl start display-manager.service 2>/dev/null || true
wait_for "the backstop's text login on tty2" 95 systemctl is-active --quiet better-os-login-fallback@tty2.service
took=$((SECONDS - started))
((took <= 90)) || fail "the backstop: expected the text login within 90 s, found ${took} s"
expect "the supervisor" failed "$(show "$SUPERVISOR" ActiveState)"
restarts="$(show "$SUPERVISOR" NRestarts)"
((restarts >= 4)) || fail "the supervisor's restarts before the backstop: expected at least 4, found $restarts"
expect "the backstop's result" success "$(show "$BACKSTOP" Result)"
expect "the text login on tty1" inactive "$(show better-os-login-fallback@tty1.service ActiveState)"
grep -qx quit "$RUN/plymouth-calls" || fail "the backstop: expected 'plymouth quit'"
grep -qF "couldn't be shown (the login supervisor couldn't run, " "$RUN/login-fallback.issue" \
    || fail "the backstop's message: expected the supervisor row, found: $(head -n1 "$RUN/login-fallback.issue" 2>/dev/null)"
[[ -e "$RUN/login-fallback.active" ]] || fail "the backstop: expected login-fallback.active"
expect "the backstop timer" active "$(show better-os-login-backstop.timer ActiveState)"
pass "a supervisor that cannot start: the backstop's text login on tty2 in ${took} s"

# Run again with a fall back running: nothing.
tty2="$(show better-os-login-fallback@tty2.service InvocationID)"
systemctl start "$BACKSTOP"
expect "the text login after a second backstop run" "$tty2" "$(show better-os-login-fallback@tty2.service InvocationID)"
# With the greeter once ready this boot: nothing.
systemctl stop better-os-login-fallback@tty2.service
rm -f "$RUN/login-fallback.active"
mkdir -p /run/better-os-greeter
: > /run/better-os-greeter/ready-seen
systemctl start "$BACKSTOP"
expect "the text login after the greeter was ready" inactive "$(show better-os-login-fallback@tty2.service ActiveState)"
pass "the backstop does nothing once the greeter was ready or a fall back runs"

rm -rf "/etc/systemd/system/$SUPERVISOR.d"
systemctl daemon-reload
echo "login-path check passed"
