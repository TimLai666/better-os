#!/usr/bin/env bash
# shellcheck disable=SC2015,SC2034 # checks read "test && pass || fail"; the AUTOSTART_* and PACKAGE_* variables are read by build-deb.sh's functions, evaluated below
# The maintainer scripts build-deb.sh writes for acting in each person's
# home (account_block): they write no file, so a TMPDIR that does not exist
# (`su` without `-`, then apt) fails nothing, on upgrade or on removal; and
# they run end to end as a real `sh`. build-deb.sh's own functions write the
# scripts into a scratch staging directory; only the header lines that name
# the machine's programs and files are pointed at scratch stand-ins before
# the scripts run. Nothing outside the scratch directory is read or written.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf -- "$WORK_DIR"' EXIT
FAILURES=0
fail() { printf 'FAIL: %s\n' "$1" >&2; FAILURES=$((FAILURES + 1)); }
pass() { printf 'ok: %s\n' "$1"; }

for function_name in account_block add_before_removal_accounts write_autostart_override_postrm; do
    definition="$(sed -n "/^$function_name() {/,/^}/p" "$ROOT_DIR/packaging/build-deb.sh")"
    [ -n "$definition" ] || {
        printf 'build-deb.sh has no %s\n' "$function_name" >&2
        exit 1
    }
    eval "$definition"
done
AUTOSTART_OVERRIDE_MARK='X-BetterOS-HiddenBy=better-touchpad'
PACKAGE_AUTOSTART_OVERRIDES='touchegg.desktop fusuma.desktop'

manager="$WORK_DIR/manager"
touchpad="$WORK_DIR/touchpad"
mkdir -p "$manager/DEBIAN" "$touchpad/DEBIAN"
printf '#!/bin/sh\nset -e\n' > "$manager/DEBIAN/prerm"
add_before_removal_accounts "$manager" better-manager
write_autostart_override_postrm "$touchpad"

for script in "$manager/DEBIAN/prerm" "$manager/DEBIAN/postinst" "$manager/DEBIAN/postrm" "$touchpad/DEBIAN/postrm"; do
    sh -n "$script" && pass "$(basename "$(dirname "$(dirname "$script")")") $(basename "$script") parses" ||
        fail "$script does not parse"
    ! grep -q mktemp "$script" && pass "$(basename "$script") writes no temporary file" || fail "$script uses mktemp"
done
sed -n 3p "$manager/DEBIAN/prerm" | grep -q 'no stamp of an older removal' &&
    pass "the prerm clears an older stamp before anything else" || fail "the stamp is not cleared first"

# One person, one home, stand-ins for the programs.
me="$(id -u)"
home="$WORK_DIR/home/a"
mkdir -p "$home/.local/state/better-os" "$home/.config/autostart" "$WORK_DIR/run"
printf 'a:x:%s:%s::%s:/bin/sh\n' "$me" "$me" "$home" > "$WORK_DIR/passwd"
printf 'UID_MIN\t%s\nUID_MAX\t%s\n' "$me" "$me" > "$WORK_DIR/login.defs"
# shellcheck disable=SC2016 # the stand-in's own $1, expanded when it runs
printf '#!/bin/sh\n[ "$1" = passwd ] && cat "%s/passwd"\n' "$WORK_DIR" > "$WORK_DIR/getent"
# shellcheck disable=SC2016 # the stand-in's own $*, expanded when it runs
printf '#!/bin/sh\necho "ran $*" >> "%s/program.log"\n' "$WORK_DIR" > "$WORK_DIR/program"
chmod +x "$WORK_DIR/getent" "$WORK_DIR/program"
point() {
    sed -e "s|^better_os_login_defs=.*|better_os_login_defs='$WORK_DIR/login.defs'|" \
        -e "s|^better_os_getent=.*|better_os_getent='$WORK_DIR/getent'|" \
        -e "s|^better_os_session_root=.*|better_os_session_root='$WORK_DIR/run-user'|" \
        -e "s|^better_os_before_removal_program=.*|better_os_before_removal_program='$WORK_DIR/program'|" \
        -e "s|^better_os_run_file=.*|better_os_run_file='$WORK_DIR/run/removal-run'|" \
        -e "s|/run/better-os/better-manager-removal-run|$WORK_DIR/run/removal-run|" \
        -e "s|systemd-tmpfiles --create|true|" \
        "$1" > "$2"
}
for script in prerm postinst postrm; do
    point "$manager/DEBIAN/$script" "$WORK_DIR/$script"
done
point "$touchpad/DEBIAN/postrm" "$WORK_DIR/touchpad-postrm"

for action in upgrade remove; do
    TMPDIR=/nonexistent sh "$WORK_DIR/prerm" "$action" 1.0 > /dev/null 2>&1 &&
        pass "prerm $action exits 0 with TMPDIR pointing nowhere" || fail "prerm $action failed with a missing TMPDIR"
done
grep -qx "ran " "$WORK_DIR/program.log" && pass "prerm remove ran the program once" || fail "prerm remove: $(cat "$WORK_DIR/program.log" 2>/dev/null)"
[ -s "$WORK_DIR/run/removal-run" ] && pass "prerm remove kept this run's stamp" || fail "no stamp"
for action in configure abort-remove; do
    TMPDIR=/nonexistent sh "$WORK_DIR/postinst" "$action" 1.0 > /dev/null 2>&1 &&
        pass "postinst $action exits 0 with TMPDIR pointing nowhere" || fail "postinst $action failed"
done
grep -qx "ran --undo" "$WORK_DIR/program.log" && pass "postinst abort-remove ran --undo" || fail "abort-remove: $(cat "$WORK_DIR/program.log")"
for action in upgrade remove purge; do
    TMPDIR=/nonexistent sh "$WORK_DIR/postrm" "$action" 1.0 > /dev/null 2>&1 &&
        pass "postrm $action exits 0 with TMPDIR pointing nowhere" || fail "postrm $action failed"
    TMPDIR=/nonexistent sh "$WORK_DIR/touchpad-postrm" "$action" 1.0 > /dev/null 2>&1 &&
        pass "the touchpad postrm $action exits 0 with TMPDIR pointing nowhere" || fail "touchpad postrm $action failed"
done
[ ! -e "$WORK_DIR/run/removal-run" ] && pass "postrm remove deleted the stamp" || fail "the stamp outlived postrm"
# A stamp from an older removal goes at the top of the next prerm.
printf 'old\n' > "$WORK_DIR/run/removal-run"
sed -n 1,4p "$WORK_DIR/prerm" > "$WORK_DIR/prerm-head"
printf 'exit 0\n' >> "$WORK_DIR/prerm-head"
sh "$WORK_DIR/prerm-head" remove
[ ! -e "$WORK_DIR/run/removal-run" ] && pass "an older stamp goes before any other fragment runs" || fail "an older stamp stayed"
exit "$FAILURES"
