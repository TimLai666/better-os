#!/usr/bin/env bash
# shellcheck disable=SC1091,SC2015,SC2016,SC2030,SC2031,SC2034 # each case sources the library in a subshell with its header variables; checks read "test && pass || fail"
# Drives packaging/maintainer/before-removal-accounts.sh (ticket 90) against
# a scratch passwd file, scratch homes, and a stand-in before-removal
# program. Nothing outside the scratch directory is read or written.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf -- "$WORK_DIR"' EXIT
FAILURES=0
fail() { printf 'FAIL: %s\n' "$1" >&2; FAILURES=$((FAILURES + 1)); }
pass() { printf 'ok: %s\n' "$1"; }

me="$(id -u)"
home_a="$WORK_DIR/home/a"
home_l="$WORK_DIR/home/l" # the home itself is a link
mkdir -p "$home_a/.local/state/better-os" "$WORK_DIR/elsewhere/.local/state/better-os" "$WORK_DIR/home"
ln -s "$WORK_DIR/elsewhere" "$home_l"
cat > "$WORK_DIR/passwd" <<PASSWD
root:x:0:0:root:/root:/bin/sh
a:x:$me:$me::$home_a:/bin/sh
l:x:$me:$me::$home_l:/bin/sh
rel:x:$me:$me::rel:/bin/sh
slash:x:$me:$me::/:/bin/sh
svc:x:120:120::$WORK_DIR/home/svc:/usr/sbin/nologin
PASSWD
printf 'UID_MIN\t%s\nUID_MAX\t%s\n' "$me" "$((me + 1))" > "$WORK_DIR/login.defs"

# The stand-in program records who ran it, where, and with what. It is
# written once here and run by the scripts below, never right after a write
# by a test thread.
cat > "$WORK_DIR/program" <<'PROGRAM'
#!/bin/sh
printf '%s|%s|%s|%s\n' "$HOME" "$PATH" "${XDG_STATE_HOME:-unset}" "$*" >> "${0%/*}/program.log"
exit 3
PROGRAM
chmod +x "$WORK_DIR/program"
cat > "$WORK_DIR/setpriv" <<'FAKE'
#!/bin/sh
printf '%s\n' "$*" >> "${0%/*}/setpriv.log"
while [ "$1" != "--" ]; do shift; done
shift
exec "$@"
FAKE
chmod +x "$WORK_DIR/setpriv"

run() {
    (
        better_os_passwd_file="$WORK_DIR/passwd"
        better_os_login_defs="$WORK_DIR/login.defs"
        better_os_setpriv="$WORK_DIR/setpriv"
        better_os_before_removal_program="${PROGRAM:-$WORK_DIR/program}"
        better_os_before_removal_leftover='.local/state/better-os/fixes-removal-undo.json'
        export XDG_STATE_HOME="$WORK_DIR/not-for-the-program"
        . "$ROOT_DIR/packaging/maintainer/accounts.sh"
        . "$ROOT_DIR/packaging/maintainer/before-removal-accounts.sh"
        "$@"
    )
}

run better_os_before_removal_prerm upgrade 1.0
[ ! -e "$WORK_DIR/program.log" ] && pass "an upgrade runs nothing" || fail "an upgrade ran the program"

run better_os_before_removal_prerm remove
[ "$(cat "$WORK_DIR/program.log")" = "$home_a|/usr/bin:/bin|unset|" ] \
    && pass "removal runs the program once, for the one usable home, with a cleared environment" \
    || fail "removal ran: $(cat "$WORK_DIR/program.log")"
pass "an exit status of 3 does not stop the removal"

rm -f "$WORK_DIR/program.log"
run better_os_before_removal_postinst configure 1.0
[ ! -e "$WORK_DIR/program.log" ] && pass "a configure runs nothing" || fail "configure ran the program"
run better_os_before_removal_postinst abort-remove
[ "$(cat "$WORK_DIR/program.log")" = "$home_a|/usr/bin:/bin|unset|--undo" ] \
    && pass "an abandoned removal runs the program with --undo" \
    || fail "abort-remove ran: $(cat "$WORK_DIR/program.log")"

leftover=".local/state/better-os/fixes-removal-undo.json"
printf '{}' > "$home_a/$leftover"
printf '{}' > "$WORK_DIR/elsewhere/$leftover"
run better_os_before_removal_postrm upgrade 1.0
[ -e "$home_a/$leftover" ] && pass "an upgrade's postrm keeps what was set aside" || fail "an upgrade deleted it"
run better_os_before_removal_postrm remove
[ ! -e "$home_a/$leftover" ] && pass "the postrm deletes what the program set aside" || fail "the set-aside file stayed"
[ -e "$WORK_DIR/elsewhere/$leftover" ] && pass "a home that is a link is not followed" || fail "a linked home was followed"
printf 'keep' > "$WORK_DIR/target"
ln -s "$WORK_DIR/target" "$home_a/$leftover"
run better_os_before_removal_postrm purge
[ -L "$home_a/$leftover" ] && [ "$(cat "$WORK_DIR/target")" = keep ] \
    && pass "a link in its place is left alone" || fail "a link was followed or removed"

rm -f "$WORK_DIR/program.log"
PROGRAM="$WORK_DIR/missing" run better_os_before_removal_prerm remove
pass "a package without the program removes quietly"
[ ! -e "$WORK_DIR/setpriv.log" ] && pass "this account's own home needs no setpriv" || fail "setpriv ran for the test's own account"
exit "$FAILURES"
