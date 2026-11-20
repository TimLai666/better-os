#!/usr/bin/env bash
# shellcheck disable=SC1091,SC2015,SC2016,SC2030,SC2031,SC2034 # each case sources the library in a subshell with its header variables; checks read "test && pass || fail"
# Drives packaging/maintainer/before-removal-accounts.sh (ticket 90) against
# a scratch passwd listing, scratch homes, and a stand-in before-removal
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
home_s="$WORK_DIR/home/s" # its program does not finish
mkdir -p "$home_a/.local/state/better-os" "$home_s" "$WORK_DIR/elsewhere/.local/state/better-os" "$WORK_DIR/home"
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
printf '#!/bin/sh\n[ "$1" = passwd ] && cat "%s/passwd"\n' "$WORK_DIR" > "$WORK_DIR/getent"
chmod +x "$WORK_DIR/getent"

# The stand-in program records who ran it, where, with what, whether it had
# a terminal and a session of its own, and what it read on standard input.
# It is written once here and run by the scripts below.
cat > "$WORK_DIR/program" <<'PROGRAM'
#!/bin/sh
case "$HOME" in
    */home/s) sleep 10 ;;
esac
input="$(cat)"
session=own
[ "$(ps -o sid= -p $$ | tr -d ' ')" = "$(cat "${0%/*}/outer.sid")" ] && session=shared
printf '%s|%s|%s|%s|%s|%s|%s\n' "$HOME" "$PATH" "${XDG_STATE_HOME:-unset}" \
    "${BETTER_OS_REMOVAL_RUN:-none}" "$*" "${input:-no-input}" "$session" >> "${0%/*}/program.log"
echo "left as you changed it"
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
RUN_FILE="$WORK_DIR/run/better-manager-removal-run"
# The session this test runs in, which the program must not be part of.
ps -o sid= -p $$ | tr -d ' ' > "$WORK_DIR/outer.sid"

# The library the generated scripts write (build-deb.sh account_block): the
# header, accounts.sh, account-step.sh, and the fragment, in that order.
make_lib() {
    {
        printf "better_os_login_defs='%s'\n" "$WORK_DIR/login.defs"
        printf "better_os_getent='%s'\n" "$WORK_DIR/getent"
        printf "better_os_setpriv='%s'\n" "$WORK_DIR/setpriv"
        printf "better_os_step_seconds='%s'\n" "${STEP_SECONDS:-30}"
        printf "better_os_before_removal_program='%s'\n" "${PROGRAM:-$WORK_DIR/program}"
        printf "better_os_before_removal_leftovers='%s'\n" \
            ".local/state/better-os/fixes-removal-undo.json .local/state/better-os/fixes-removal-undo.json.tmp"
        printf "better_os_run_file='%s'\n" "$RUN_FILE"
        cat "$ROOT_DIR/packaging/maintainer/accounts.sh"
        cat "$ROOT_DIR/packaging/maintainer/account-step.sh"
        cat "$ROOT_DIR/packaging/maintainer/before-removal-accounts.sh"
    } > "$WORK_DIR/lib.sh"
}

run() {
    make_lib
    (
        export XDG_STATE_HOME="$WORK_DIR/not-for-the-program"
        better_os_account_lib="$WORK_DIR/lib.sh"
        . "$better_os_account_lib"
        "$@"
    ) <<< "a terminal's input"
}

run better_os_before_removal_prerm upgrade 1.0
[ ! -e "$WORK_DIR/program.log" ] && pass "an upgrade runs nothing" || fail "an upgrade ran the program"

output="$(run better_os_before_removal_prerm remove 2>&1)"
stamp="$(cat "$RUN_FILE")"
[ -n "$stamp" ] && pass "the prerm keeps a stamp for this dpkg run" || fail "no stamp was kept"
[ "$(cat "$WORK_DIR/program.log")" = "$home_a|/usr/bin:/bin|unset|$stamp||no-input|own" ] \
    && pass "removal runs the program once, for the one usable home, with a cleared environment, the stamp, no input, and a session of its own" \
    || fail "removal ran: $(cat "$WORK_DIR/program.log")"
printf '%s\n' "$output" | grep -qx "Better OS (a): left as you changed it" \
    && pass "what the program says reaches the person running apt, through a pipe" \
    || fail "the program's words were: $output"
pass "an exit status of 3 does not stop the removal"

rm -f "$WORK_DIR/program.log"
run better_os_before_removal_postinst configure 1.0
[ ! -e "$WORK_DIR/program.log" ] && pass "a configure runs nothing" || fail "configure ran the program"
run better_os_before_removal_postinst abort-remove > /dev/null
[ "$(cat "$WORK_DIR/program.log")" = "$home_a|/usr/bin:/bin|unset|$stamp|--undo|no-input|own" ] \
    && pass "an abandoned removal runs the program with --undo and the same stamp" \
    || fail "abort-remove ran: $(cat "$WORK_DIR/program.log")"
[ ! -e "$RUN_FILE" ] && pass "the stamp goes with the abandoned removal" || fail "the stamp stayed"
rm -f "$WORK_DIR/program.log"
run better_os_before_removal_postinst abort-remove > /dev/null
grep -q "|none|--undo|" "$WORK_DIR/program.log" \
    && pass "with no stamp of this run, --undo is given none (and uses nothing set aside)" \
    || fail "abort-remove without a stamp ran: $(cat "$WORK_DIR/program.log")"

leftover=".local/state/better-os/fixes-removal-undo.json"
printf '{}' > "$home_a/$leftover"
printf 'half' > "$home_a/$leftover.tmp"
printf '{}' > "$WORK_DIR/elsewhere/$leftover"
printf 'x\n' > "$RUN_FILE"
run better_os_before_removal_postrm upgrade 1.0
[ -e "$home_a/$leftover" ] && pass "an upgrade's postrm keeps what was set aside" || fail "an upgrade deleted it"
run better_os_before_removal_postrm remove
[ ! -e "$home_a/$leftover" ] && [ ! -e "$home_a/$leftover.tmp" ] \
    && pass "the postrm deletes what the program set aside, and a half-written copy" \
    || fail "something set aside stayed"
[ ! -e "$RUN_FILE" ] && pass "the postrm deletes the stamp" || fail "the stamp stayed"
[ -e "$WORK_DIR/elsewhere/$leftover" ] && pass "a home that is a link is not followed" || fail "a linked home was followed"
printf 'keep' > "$WORK_DIR/target"
ln -s "$WORK_DIR/target" "$home_a/$leftover"
run better_os_before_removal_postrm purge
[ -L "$home_a/$leftover" ] && [ "$(cat "$WORK_DIR/target")" = keep ] \
    && pass "a link in its place is left alone" || fail "a link was followed or removed"

# A program that does not finish: killed after the step's time, the account
# named, the next account still done.
printf 's:x:%s:%s::%s:/bin/sh\n' "$me" "$me" "$home_s" > "$WORK_DIR/passwd.first"
cat "$WORK_DIR/passwd" >> "$WORK_DIR/passwd.first"
mv "$WORK_DIR/passwd.first" "$WORK_DIR/passwd"
rm -f "$WORK_DIR/program.log"
started="$(date +%s)"
warning="$(STEP_SECONDS=1 run better_os_before_removal_prerm remove 2>&1 >/dev/null)"
took=$(($(date +%s) - started))
[ "$took" -lt 8 ] && pass "a program that hangs holds the removal for its time limit only (${took}s)" || fail "the removal waited ${took}s"
printf '%s\n' "$warning" | grep -q "s ($home_s) did not answer within 1 seconds" \
    && pass "the account that did not answer is named" || fail "no warning named it: $warning"
grep -q "^$home_a|" "$WORK_DIR/program.log" && pass "the next account is still done" || fail "a hung account stopped the others"

rm -f "$WORK_DIR/program.log"
PROGRAM="$WORK_DIR/missing" run better_os_before_removal_prerm remove
pass "a package without the program removes quietly"
[ ! -e "$WORK_DIR/setpriv.log" ] && pass "this account's own home needs no setpriv" || fail "setpriv ran for the test's own account"
exit "$FAILURES"
