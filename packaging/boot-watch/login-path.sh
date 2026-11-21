#!/usr/bin/env bash
# The Better OS login supervisor and its backstops against a real systemd,
# logind and D-Bus (design/login-screen-spec.md section 5.10, ticket 267):
#
#   A. a stand-in greeter that never becomes ready, whose session runs in a
#      scope of its own and ignores SIGTERM, as pam_systemd and a stuck
#      compositor would have it: three attempts, each session killed before
#      the next start, then the text login on tty8 within 80 s; the journal
#      lines, the record, the message, the bus interface and the status
#      command; the backstop timer active from the supervisor's first start;
#      getty on tty1 and plymouth-quit standing aside while this is the
#      display manager; the supervisor untouched by every stop, start and
#      restart of the greeter's unit;
#   B. the supervisor killed in the middle of its second attempt: it
#      continues from /run/better-os/login-path.json with the same T0, and
#      the backstop does not run;
#   D. the display manager stopped and started again: a new appearance;
#   E. the text login refused on tty8: it moves to tty9, and is marked only
#      there;
#   C. a supervisor that cannot start: after five starts the backstop for a
#      failed supervisor gives the text login on tty9 within 90 s, and does
#      nothing while a text login runs or a greeter lives.
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
FAILED_BACKSTOP=better-os-login-backstop-failed.service
TIMER=better-os-login-backstop.timer
TTY8=better-os-login-fallback@tty8.service
TTY9=better-os-login-fallback@tty9.service
RUN=/run/better-os
RECORDS=/var/lib/better-os/boot-watch

dump() {
    echo "--- journal of the login path ---" >&2
    journalctl -b --no-pager -o short-monotonic \
        -u "$SUPERVISOR" -u "$GREETER" -u "$BACKSTOP" -u "$FAILED_BACKSTOP" -u "$TIMER" \
        -u 'better-os-login-fallback@*' -u 'session-g*.scope' 2>/dev/null | tail -n 150 >&2 || true
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
mark() { cursor="$(journalctl -b -n 1 -o cat --show-cursor --no-pager | sed -n 's/^-- cursor: //p')"; }
journal() {
    if [[ -n "$cursor" ]]; then
        journalctl -b --no-pager -o cat --after-cursor="$cursor" -u "$1" 2>/dev/null
    else
        journalctl -b --no-pager -o cat -u "$1" 2>/dev/null
    fi
}
retried() { journal "$SUPERVISOR" | grep -q 'RESULT=retried'; }
# The journal is written a moment after the line is printed.
has_line() { journal "$SUPERVISOR" | grep -qxF "$1"; }
active() { systemctl is-active --quiet "$1"; }
restarted() { (($(show "$SUPERVISOR" NRestarts) >= 1)); }
seconds_between() { echo $((($2 - $1) / 1000000)); }
live_scopes() { systemctl list-units --no-legend --plain 'session-g*.scope' 2>/dev/null | grep -c . || true; }

# --- The stand-ins -----------------------------------------------------------

# A privileged container sees the host's terminals: every terminal the
# supervisor or chvt opens here is /dev/null, so nothing reaches them.
systemctl stop getty@tty1.service 2>/dev/null || true
for tty in tty0 tty1 tty8 tty9; do
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
# sandbox lets it write (the directory the package's tmpfiles entry makes).
mkdir -p "$RUN"
# shellcheck disable=SC2016 # "$*" belongs to the stand-in, not to this script
printf '#!/bin/sh\necho "$*" >> %s/plymouth-calls\n' "$RUN" > /usr/bin/plymouth
chmod 755 /usr/bin/plymouth
# Ubuntu's plymouth-quit.service, which the package's drop-in sets aside.
printf '%s\n' '[Unit]' 'Description=Stand-in for Ubuntu'"'"'s plymouth-quit.service' '[Service]' \
    'Type=oneshot' 'RemainAfterExit=yes' 'ExecStart=/usr/bin/plymouth quit' \
    > /etc/systemd/system/plymouth-quit.service

install -m 0755 /login/better-os-login-supervisor /usr/sbin/better-os-login-supervisor
install -m 0755 /login/better-os-login-status /usr/bin/better-os-login-status
for unit in "$SUPERVISOR" "$GREETER" "$BACKSTOP" "$FAILED_BACKSTOP" "$TIMER" \
    better-os-login-fallback@.service; do
    install -m 0644 "/login/greeter/$unit" "/usr/lib/systemd/system/$unit"
done
for dropin in getty@tty1.service.d plymouth-quit.service.d; do
    install -D -m 0644 "/login/greeter/$dropin/better-os-login.conf" \
        "/usr/lib/systemd/system/$dropin/better-os-login.conf"
done
install -m 0644 /login/greeter/org.betteros.LoginSupervisor1.conf \
    /usr/share/dbus-1/system.d/org.betteros.LoginSupervisor1.conf
install -D -m 0644 /login/greeter/login-supervisor-failed.issue \
    /usr/share/better-os/login-supervisor-failed.issue

# The greeter's account, as the package's sysusers entry makes it.
id _better-greeter >/dev/null 2>&1 \
    || useradd --system --no-create-home --shell /usr/sbin/nologin _better-greeter
greeter_uid="$(id -u _better-greeter)"

# The greeter never becomes ready: no greetd, no greeter session in logind,
# no heartbeat. Its "session" runs in a session scope of its own, as
# pam_systemd puts greetd's, and ignores SIGTERM, as a stuck compositor
# would: the unit's stop never reaches it, only the supervisor's kill does.
printf '%s\n' '#!/bin/sh' \
    "systemd-run --scope --quiet --slice=user-$greeter_uid.slice --unit=session-g\$\$.scope \\" \
    "    setpriv --reuid=_better-greeter --regid=_better-greeter --clear-groups \\" \
    "    /bin/sh -c 'trap \"\" TERM; while :; do sleep 1; done' &" \
    'wait' > /login/greeter-stand-in.sh
chmod 755 /login/greeter-stand-in.sh
mkdir -p "/etc/systemd/system/$GREETER.d"
printf '[Service]\nExecStart=\nExecStart=/login/greeter-stand-in.sh\n' \
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
    "/usr/lib/systemd/system/$BACKSTOP" "/usr/lib/systemd/system/$FAILED_BACKSTOP" \
    "/usr/lib/systemd/system/$TIMER" 2>&1)" \
    || fail "systemd-analyze verify: expected the units accepted, found: $verify"

reset() {
    systemctl stop display-manager.service "$GREETER" "$BACKSTOP" "$FAILED_BACKSTOP" "$TIMER" \
        "$TTY8" "$TTY9" 'session-g*.scope' 2>/dev/null || true
    systemctl unmask "$TTY8" 2>/dev/null || true
    systemctl reset-failed 2>/dev/null || true
    rm -f "$RUN/login-path.json" "$RUN/login-fallback.issue" "$RUN/login-fallback.active" \
        "$RUN/login-held" "$RUN/greeter-mode" "$RUN/plymouth-calls"
    rm -rf /run/better-os-greeter "$RECORDS/evidence" "$RECORDS/summary.json"
}
greeter_starts() { journal "$GREETER" | grep -c '^Started ' || true; }

# --- The boot transaction: getty on tty1 and plymouth-quit stand aside ---------

systemctl start getty@tty1.service plymouth-quit.service || true
expect "getty@tty1 while this is the display manager" inactive "$(show getty@tty1.service ActiveState)"
expect "plymouth-quit while this is the display manager" inactive "$(show plymouth-quit.service ActiveState)"
[[ ! -e "$RUN/plymouth-calls" ]] || fail "plymouth-quit quit the splash: $(cat "$RUN/plymouth-calls")"
# With another display manager chosen they run as before.
ln -sf /lib/systemd/system/gdm3.service /etc/systemd/system/display-manager.service
systemctl start plymouth-quit.service
expect "plymouth-quit with another display manager" active "$(show plymouth-quit.service ActiveState)"
systemctl stop plymouth-quit.service
ln -sf "/usr/lib/systemd/system/$SUPERVISOR" /etc/systemd/system/display-manager.service
systemctl daemon-reload
rm -f "$RUN/plymouth-calls"
pass "getty on tty1 and plymouth-quit stand aside while this is the display manager, and only then"

# --- A. The greeter never becomes ready -------------------------------------

reset
mark
started=$SECONDS
systemctl start display-manager.service
t0="$(show "$SUPERVISOR" InactiveExitTimestampMonotonic)"
invocation="$(show "$SUPERVISOR" InvocationID)"
timer_active="$(show "$TIMER" ActiveEnterTimestampMonotonic)"
((timer_active > 0 && timer_active <= t0 && t0 - timer_active < 2000000)) \
    || fail "the backstop timer: expected active right before the supervisor's start ($t0), found $timer_active"
wait_for "the text login on tty8" 85 active "$TTY8"
text_login="$(show "$TTY8" ActiveEnterTimestampMonotonic)"
after="$(seconds_between "$t0" "$text_login")"
((after + 3 <= 80)) || fail "the text login: expected to accept input within 80 s of T0, found $((after + 3)) s"
pass "the greeter never ready: the text login on tty8 ${after} s after T0 ($((SECONDS - started)) s here)"

for line in "BETTER_OS_LOGIN_STAGE=5 RESULT=retried REASON=no-greeter-session" \
    "BETTER_OS_LOGIN_STAGE=5 RESULT=ok REASON=session-killed" \
    "BETTER_OS_LOGIN_STAGE=5 RESULT=fell-back REASON=no-greeter-session"; do
    wait_for "the journal line '$line'" 5 has_line "$line"
done
expect "the greeter's starts" 3 "$(greeter_starts)"
expect "greeter sessions left running" 0 "$(live_scopes)"
expect "the third attempt's mode" safe "$(cat "$RUN/greeter-mode" 2>/dev/null)"
expect "the supervisor's restarts" 0 "$(show "$SUPERVISOR" NRestarts)"
expect "the supervisor after the fall back" active "$(show "$SUPERVISOR" ActiveState)"
wait_for "the fall back marker" 5 test -e "$RUN/login-fallback.active"
expect "the fall back marker" tty8 "$(cat "$RUN/login-fallback.active" 2>/dev/null)"
[[ -e "$RUN/login-held" ]] || fail "login-held: expected after the fall back, not found"
grep -qx deactivate "$RUN/plymouth-calls" || fail "plymouth: expected 'deactivate', found $(tr '\n' ' ' < "$RUN/plymouth-calls")"
grep -qx quit "$RUN/plymouth-calls" || fail "plymouth: expected 'quit', found $(tr '\n' ' ' < "$RUN/plymouth-calls")"
grep -qF "couldn't be shown (the sign-in service didn't start, " "$RUN/login-fallback.issue" \
    || fail "the message: expected stage 5's words, found: $(head -n1 "$RUN/login-fallback.issue")"
expect "the state file's mode" 644 "$(stat -c %a "$RUN/login-path.json")"
pass "journal lines, three attempts (the third in safe mode), every session killed, Plymouth, the message"

shopt -s nullglob
events=("$RECORDS"/evidence/[0-9]*-*.json)
expect "the event records" 1 "${#events[@]}"
grep -q '"result": "text_login_used"' "${events[0]}" || fail "the record: expected text_login_used, found $(cat "${events[0]}")"
grep -q '"stage": "sign_in_service"' "${events[0]}" || fail "the record: expected stage sign_in_service"
grep -q '"tty": "tty8"' "${events[0]}" || fail "the record: expected tty8"
grep -q '"display_manager": "better-os-login-supervisor.service"' "${events[0]}" || fail "the record names another display manager"
expect "the record's owner and mode" 640:root:adm "$(stat -c %a:%U:%G "${events[0]}")"
expect "the summary's mode" 644 "$(stat -c %a "$RECORDS/summary.json")"
grep -q text_login_used "$RECORDS/summary.json" || fail "the summary: expected the event"
pass "the record, in ADR 0033's place and format"

reply="$(setpriv --reuid=nobody --regid=nogroup --clear-groups \
    busctl call org.betteros.LoginSupervisor1 /org/betteros/LoginSupervisor1 \
    org.betteros.LoginSupervisor1 LastRecord 2>&1)" || fail "LastRecord as nobody: $reply"
grep -q 'text_login' <<<"$reply" || fail "LastRecord: expected the text login, found $reply"
if setpriv --reuid=nobody --regid=nogroup --clear-groups busctl call org.betteros.LoginSupervisor1 \
    /org/betteros/LoginSupervisor1 org.freedesktop.DBus.Properties GetAll s \
    org.betteros.LoginSupervisor1 >/dev/null 2>&1; then
    fail "the bus policy: expected only LastRecord open to nobody, found Properties open too"
fi
status="$(TERM=linux LANG=zh_TW.UTF-8 setpriv --reuid=nobody --regid=nogroup --clear-groups \
    better-os-login-status)" || fail "better-os-login-status: $status"
grep -qxF "Now: using the text sign-in (tty8)" <<<"$status" \
    || fail "better-os-login-status on the console: expected English and the fall back, found: $status"
grep -q "read from its files" <<<"$status" && fail "better-os-login-status did not reach the supervisor: $status"
pass "org.betteros.LoginSupervisor1 answers anyone, and the status command prints English on the console"

for step in "start $GREETER" "restart $GREETER" "stop $GREETER" "start $TTY9" "stop $TTY9" \
    "start plymouth-quit.service"; do
    # shellcheck disable=SC2086
    systemctl $step || true
    expect "the supervisor after 'systemctl $step'" "$invocation" "$(show "$SUPERVISOR" InvocationID)"
done
expect "the supervisor after the greeter's unit was stopped and restarted" active "$(show "$SUPERVISOR" ActiveState)"
pass "starting, restarting and stopping the greeter's unit and plymouth-quit never reach the supervisor"

# --- B. Killed in the middle of the second attempt -------------------------

reset
mark
systemctl start display-manager.service
t0="$(show "$SUPERVISOR" InactiveExitTimestampMonotonic)"
wait_for "the first retry" 40 retried
sleep 2
# By its process id: `systemctl kill` also signals the cgroup, which some
# runners' kernels refuse.
kill -KILL "$(show "$SUPERVISOR" MainPID)"
wait_for "the supervisor's restart" 10 restarted
wait_for "the text login on tty8" 85 active "$TTY8"
text_login="$(show "$TTY8" ActiveEnterTimestampMonotonic)"
after="$(seconds_between "$t0" "$text_login")"
# The restart costs its own downtime (about a second) on top of the budget.
((after + 3 <= 82)) || fail "after a restart: expected the text login within 82 s of the first start, found $((after + 3)) s"
expect "T0 kept across the restart" "$t0" \
    "$(grep -o '"t0":[0-9]*' "$RUN/login-path.json" | cut -d: -f2)"
wait_for "the journal line of the restart" 5 \
    has_line "BETTER_OS_LOGIN_STAGE=supervisor RESULT=ok REASON=restarted"
expect "the greeter's starts across the restart" 3 "$(greeter_starts)"
expect "greeter sessions left running" 0 "$(live_scopes)"
# One crash is the supervisor's own restart, never the backstop's: with
# RestartMode=direct, OnFailure= fires only once systemd gives up.
expect "the backstops' runs after one crash" 0 \
    "$( (journal "$BACKSTOP"; journal "$FAILED_BACKSTOP") | grep -c . || true)"
expect "the text login on tty9 after one crash" inactive "$(show "$TTY9" ActiveState)"
pass "killed mid-attempt: restarted, continued from its state file, the text login ${after} s after the first T0"

# --- D. The display manager stopped and started again: a new appearance -------

systemctl stop display-manager.service
sleep 1
mark
systemctl start display-manager.service
t0_again="$(show "$SUPERVISOR" InactiveExitTimestampMonotonic)"
wait_for "the journal line of a start again" 5 \
    has_line "BETTER_OS_LOGIN_STAGE=supervisor RESULT=ok REASON=started-again"
expect "T0 of the new appearance" "$t0_again" \
    "$(grep -o '"t0":[0-9]*' "$RUN/login-path.json" | cut -d: -f2)"
grep -q '"appearance":2' "$RUN/login-path.json" || fail "the state: expected the second appearance, found $(cat "$RUN/login-path.json")"
greeter_started() { (($(greeter_starts) >= 1)); }
wait_for "the greeter started again" 20 greeter_started
pass "a display manager stopped and started again begins a new appearance"

# --- E. The text login refused on tty8 ---------------------------------------

reset
systemctl mask "$TTY8"
mark
systemctl start display-manager.service
t0="$(show "$SUPERVISOR" InactiveExitTimestampMonotonic)"
wait_for "the text login on tty9" 90 active "$TTY9"
text_login="$(show "$TTY9" ActiveEnterTimestampMonotonic)"
after="$(seconds_between "$t0" "$text_login")"
# The refused terminal costs its 3 s on top of the budget.
((after + 3 <= 83)) || fail "tty8 refused: expected the text login on tty9 within 83 s, found $((after + 3)) s"
wait_for "the journal line of the refused terminal" 5 \
    has_line "BETTER_OS_LOGIN_STAGE=F RESULT=retried REASON=text-login-not-active"
wait_for "the fall back marker" 5 test -e "$RUN/login-fallback.active"
expect "the fall back marker" tty9 "$(cat "$RUN/login-fallback.active")"
systemctl unmask "$TTY8"
pass "tty8 refused: the text login on tty9 ${after} s after T0, marked only there"

# --- C. A supervisor that cannot start ----------------------------------------

reset
mkdir -p "/etc/systemd/system/$SUPERVISOR.d"
printf '[Service]\nExecStart=\nExecStart=/bin/false\n' > "/etc/systemd/system/$SUPERVISOR.d/broken.conf"
systemctl daemon-reload
mark
started=$SECONDS
systemctl start display-manager.service 2>/dev/null || true
wait_for "the backstop's text login on tty9" 95 active "$TTY9"
took=$((SECONDS - started))
((took <= 90)) || fail "the backstop: expected the text login within 90 s, found ${took} s"
expect "the supervisor" failed "$(show "$SUPERVISOR" ActiveState)"
restarts="$(show "$SUPERVISOR" NRestarts)"
((restarts >= 4)) || fail "the supervisor's restarts before the backstop: expected at least 4, found $restarts"
expect "the failed supervisor's backstop" success "$(show "$FAILED_BACKSTOP" Result)"
expect "the text login on tty8" inactive "$(show "$TTY8" ActiveState)"
grep -qx quit "$RUN/plymouth-calls" || fail "the backstop: expected 'plymouth quit'"
grep -qF "couldn't be shown (the login supervisor couldn't run, " "$RUN/login-fallback.issue" \
    || fail "the backstop's message: expected the supervisor row, found: $(head -n1 "$RUN/login-fallback.issue" 2>/dev/null)"
[[ -e "$RUN/login-fallback.active" ]] || fail "the backstop: expected login-fallback.active"
expect "the backstop timer" active "$(show "$TIMER" ActiveState)"
pass "a supervisor that cannot start: the backstop's text login on tty9 in ${took} s"

# The present, not the boot's history: with a text login running, nothing.
tty9="$(show "$TTY9" InvocationID)"
rm -f "$RUN/login-fallback.active"
# A condition that is not met skips the unit; systemctl's own status is not
# what is checked.
systemctl start "$FAILED_BACKSTOP" || true
expect "the text login after a second run with one running" "$tty9" "$(show "$TTY9" InvocationID)"
# With a greeter alive now (its heartbeat fresh): nothing.
systemctl stop "$TTY9"
mkdir -p /run/better-os-greeter
echo ready > /run/better-os-greeter/alive
systemctl start "$FAILED_BACKSTOP" || true
expect "the text login with a greeter alive" inactive "$(show "$TTY9" ActiveState)"
# The heartbeat old, nobody signed in, no text login: it runs, whatever this
# boot saw before (the greeter was ready once).
touch -d '-1 minute' /run/better-os-greeter/alive
: > /run/better-os-greeter/ready-seen
: > "$RUN/login-held"
systemctl start "$FAILED_BACKSTOP" || true
wait_for "the failed supervisor's backstop after the property once held" 10 active "$TTY9"
# The timer's backstop keeps to the boot's history: with the greeter ready
# once this boot, nothing.
systemctl stop "$TTY9"
rm -f "$RUN/login-fallback.active"
systemctl start "$BACKSTOP" || true
expect "the timer's backstop after the greeter was ready" inactive "$(show "$TTY9" ActiveState)"
pass "the failed supervisor's backstop looks at the present, the timer's at the boot"

rm -rf "/etc/systemd/system/$SUPERVISOR.d"
systemctl daemon-reload
echo "login-path check passed"
