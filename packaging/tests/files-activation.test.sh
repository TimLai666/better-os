#!/usr/bin/env bash
# shellcheck disable=SC1091,SC2015,SC2016,SC2030,SC2031,SC2034,SC2069 # each case runs the library in a subshell with its header variables; checks read "test && pass || fail"; single quotes hold the stand-ins' own expansions and hostile names; `2>&1 >/dev/null` captures standard error alone, on purpose; PACKAGE_* are read by build-deb.sh's functions, evaluated below
# Drives packaging/maintainer/files-activation.sh (ticket 129, ADR 0041)
# through the account library it runs in (accounts.sh, account-step.sh),
# against a scratch passwd listing and scratch homes, then runs the postrm
# build-deb.sh itself writes. Nothing outside the scratch directory is read
# or written.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf -- "$WORK_DIR"' EXIT
FAILURES=0
fail() { printf 'FAIL: %s\n' "$1" >&2; FAILURES=$((FAILURES + 1)); }
pass() { printf 'ok: %s\n' "$1"; }

me="$(id -u)"
NAME='org.freedesktop.FileManager1'
EXEC='/usr/bin/better-files --service'
file() { printf '%s/.local/share/dbus-1/services/%s.service' "$1" "$NAME"; }
aside() { printf '%s/.local/share/dbus-1/services/.%s.service.before-better-os' "$1" "$NAME"; }
ours() { printf '[D-BUS Service]\nName=%s\nExec=%s\n' "$NAME" "$EXEC" > "$1"; }
theirs() { printf '[D-BUS Service]\nName=%s\nExec=/opt/other --dbus\n' "$NAME" > "$1"; }

home_a="$WORK_DIR/home/a" # Better Defaults' file: removed, empty directories go
home_b="$WORK_DIR/home/b" # the person's own file: kept
home_c="$WORK_DIR/home/c" # ~/.local/share is a link: not followed
home_d="$WORK_DIR/home/d" # a link where the file would be: kept
home_e="$WORK_DIR/home/e" # Better Defaults' file with a sibling: directory kept
home_f="$WORK_DIR/home/f" # a FIFO where the file would be: never opened
home_g="$WORK_DIR/home/g" # Better Defaults' file and the person's kept aside: theirs back
home_h="$WORK_DIR/home/h" # the person's file and one kept aside: both stay
home_k="$WORK_DIR/home/k" # only the kept-aside file is left: it is back
setup_homes() {
    rm -rf -- "$WORK_DIR/home" "$WORK_DIR/linked-share" "$WORK_DIR/elsewhere.service"
    for home in "$home_a" "$home_b" "$home_d" "$home_e" "$home_f" "$home_g" "$home_h" "$home_k"; do
        mkdir -p "$home/.local/share/dbus-1/services"
    done
    mkdir -p "$home_c/.local" "$WORK_DIR/linked-share/dbus-1/services"
    ln -s "$WORK_DIR/linked-share" "$home_c/.local/share"
    ours "$(file "$home_a")"
    theirs "$(file "$home_b")"
    ours "$WORK_DIR/linked-share/dbus-1/services/$NAME.service"
    ours "$WORK_DIR/elsewhere.service"
    ln -s "$WORK_DIR/elsewhere.service" "$(file "$home_d")"
    ours "$(file "$home_e")"
    printf 'x\n' > "$home_e/.local/share/dbus-1/services/other.service"
    mkfifo "$(file "$home_f")"
    ours "$(file "$home_g")"
    theirs "$(aside "$home_g")"
    chmod 0640 "$(aside "$home_g")"
    theirs "$(file "$home_h")"
    printf 'older\n' > "$(aside "$home_h")"
    theirs "$(aside "$home_k")"
}
write_passwd() {
    {
        printf 'root:x:0:0:root:/root:/bin/sh\n'
        for home in "$home_a" "$home_b" "$home_c" "$home_d" "$home_e" "$home_f" "$home_g" "$home_h" "$home_k"; do
            printf '%s:x:%s:%s::%s:/bin/sh\n' "${home##*/}" "$me" "$me" "$home"
        done
        printf 'svc:x:120:120::%s/home/svc:/usr/sbin/nologin\n' "$WORK_DIR"
    } > "$WORK_DIR/passwd"
}
printf 'UID_MIN\t%s\nUID_MAX\t%s\n' "$me" "$((me + 1))" > "$WORK_DIR/login.defs"
printf '#!/bin/sh\n[ "$1" = passwd ] && cat "%s/passwd"\n' "$WORK_DIR" > "$WORK_DIR/getent"
# The stand-in setsid records each step it starts and, for the home named
# "s", does not finish; otherwise it hands over to the real one.
mkdir -p "$WORK_DIR/bin"
real_setsid="$(command -v setsid)"
cat > "$WORK_DIR/bin/setsid" <<FAKE
#!/bin/sh
printf '%s\n' "\$*" >> "$WORK_DIR/setsid.log"
case "\$*" in
    *"/home/s "*) sleep 10 ;;
esac
exec "$real_setsid" "\$@"
FAKE
chmod +x "$WORK_DIR/getent" "$WORK_DIR/bin/setsid"

# The library build-deb.sh's account_block writes: the header,
# accounts.sh, account-step.sh, and the fragment, in that order.
make_lib() {
    {
        printf "better_os_login_defs='%s'\n" "$WORK_DIR/login.defs"
        printf "better_os_getent='%s'\n" "$WORK_DIR/getent"
        printf "better_os_setpriv='%s'\n" "$WORK_DIR/no-setpriv"
        printf "better_os_mounts='%s'\n" /proc/self/mounts
        printf "better_os_session_root='%s'\n" "$WORK_DIR/run-user"
        printf "better_os_total_seconds='%s'\n" "${TOTAL_SECONDS:-120}"
        printf "better_os_step_seconds='%s'\n" "${STEP_SECONDS:-30}"
        printf "better_os_activation_name='%s'\n" "$NAME"
        printf "better_os_activation_exec='%s'\n" "$EXEC"
        cat "$ROOT_DIR/packaging/maintainer/accounts.sh"
        cat "$ROOT_DIR/packaging/maintainer/account-step.sh"
        cat "$ROOT_DIR/packaging/maintainer/files-activation.sh"
    } > "$WORK_DIR/lib.sh"
}
run() {
    make_lib
    (
        export PATH="$WORK_DIR/bin:$PATH"
        better_os_account_library="$(cat "$WORK_DIR/lib.sh")"
        eval "$better_os_account_library"
        better_os_forget_activation_files "$@"
    ) <<< "a terminal's input"
}

setup_homes
write_passwd
run upgrade 1.0
[ -e "$(file "$home_a")" ] && [ -e "$(aside "$home_g")" ] && [ ! -e "$WORK_DIR/setsid.log" ] &&
    pass "an upgrade runs nothing and removes nothing" || fail "an upgrade changed something"

started="$(date +%s)"
output="$(STEP_SECONDS=5 run remove 2>&1)"
took=$(($(date +%s) - started))
[ ! -e "$(file "$home_a")" ] && pass "removal deletes the file Better Defaults wrote" || fail "the file stayed"
[ ! -e "$home_a/.local/share/dbus-1" ] && [ -d "$home_a/.local/share" ] &&
    pass "the directories it left empty go, and no further" || fail "the empty directories: $(ls -a "$home_a/.local/share")"
[ "$(cat "$(file "$home_b")")" = "$(theirs /dev/stdout)" ] && pass "the person's own file stays" || fail "the person's own file was changed"
[ -e "$WORK_DIR/linked-share/dbus-1/services/$NAME.service" ] && pass "a symlinked ~/.local/share is not followed" || fail "a linked directory was followed"
[ -L "$(file "$home_d")" ] && [ -e "$WORK_DIR/elsewhere.service" ] && pass "a symbolic link is not followed or removed" || fail "a link was followed"
[ ! -e "$(file "$home_e")" ] && [ -e "$home_e/.local/share/dbus-1/services/other.service" ] && pass "a directory with other files keeps them" || fail "other services were touched"
[ -p "$(file "$home_f")" ] && [ "$took" -lt 5 ] && pass "a FIFO in place of the file is neither opened nor removed (${took}s)" ||
    fail "the FIFO: took ${took}s, still a FIFO: $([ -p "$(file "$home_f")" ] && echo yes || echo no)"
[ "$(cat "$(file "$home_g")")" = "$(theirs /dev/stdout)" ] && [ ! -e "$(aside "$home_g")" ] &&
    [ "$(stat -c %a "$(file "$home_g")")" = 640 ] &&
    pass "the person's file kept aside is back in place, with its mode" || fail "the kept-aside file: $(ls -la "$home_g/.local/share/dbus-1/services")"
[ "$(cat "$(aside "$home_h")")" = older ] && [ "$(cat "$(file "$home_h")")" = "$(theirs /dev/stdout)" ] &&
    pass "a kept-aside file stays when the person's own file is in place" || fail "a person's file was replaced"
[ -f "$(file "$home_k")" ] && [ ! -e "$(aside "$home_k")" ] && pass "a kept-aside file with nothing in its place comes back" || fail "the lone kept-aside file stayed aside"
printf '%s\n' "$output" | grep -qx "Better OS (g): your own $NAME.service is back in use" &&
    pass "the person running apt is told whose file came back" || fail "output: $output"
[ "$(grep -c '^env -i PATH=/usr/bin:/bin LC_ALL=C sh -c' "$WORK_DIR/setsid.log")" -eq 9 ] &&
    pass "each person's step runs in a new session with a cleared environment" || fail "setsid: $(cat "$WORK_DIR/setsid.log")"
[ ! -e "$WORK_DIR/no-setpriv" ] && pass "this account's own homes need no setpriv" || fail "setpriv ran"
run purge 1.0 > /dev/null 2>&1
pass "purge with nothing left is quiet"

# A step that does not finish: cut off after the step's time, the account
# named, the next account still done.
setup_homes
mkdir -p "$WORK_DIR/home/s"
{
    printf 's:x:%s:%s::%s:/bin/sh\n' "$me" "$me" "$WORK_DIR/home/s"
    printf 'a:x:%s:%s::%s:/bin/sh\n' "$me" "$me" "$home_a"
} > "$WORK_DIR/passwd"
started="$(date +%s)"
warning="$(STEP_SECONDS=1 run remove 2>&1 >/dev/null)"
took=$(($(date +%s) - started))
[ "$took" -lt 8 ] && pass "a step that hangs holds the removal for its time limit only (${took}s)" || fail "the removal waited ${took}s"
printf '%s\n' "$warning" | grep -q "s ($WORK_DIR/home/s) did not answer within 1 seconds" &&
    pass "the account that did not answer is named" || fail "no warning named it: $warning"
[ ! -e "$(file "$home_a")" ] && pass "the next account is still done" || fail "a hung account stopped the others"

# Hostile account names from the directory: each is skipped and said, and
# nothing in them runs.
setup_homes
mkdir -p "$WORK_DIR/home/hostile/.local/share/dbus-1/services"
ours "$(file "$WORK_DIR/home/hostile")"
printf 'a:x:%s:%s::%s:/bin/sh\n' "$me" "$me" "$home_a" > "$WORK_DIR/passwd"
for hostile in 'x/;e touch PWNED #' '-rf' 'a b' '$(touch PWNED)' 'x;touch PWNED'; do
    printf '%s:x:%s:%s::%s:/bin/sh\n' "$hostile" "$me" "$me" "$WORK_DIR/home/hostile" >> "$WORK_DIR/passwd"
done
warning="$(cd "$WORK_DIR" && run remove 2>&1 >/dev/null)"
[ ! -e "$WORK_DIR/PWNED" ] && [ ! -e "$WORK_DIR/home/hostile/PWNED" ] && pass "nothing in a hostile account name runs" ||
    fail "a hostile name ran a command"
[ -e "$(file "$WORK_DIR/home/hostile")" ] && pass "an account with a hostile name is skipped" || fail "a hostile name reached the step"
printf '%s\n' "$warning" | grep -q "5 accounts whose names are not plain names were skipped" &&
    pass "the skipped accounts are counted for the person running apt" || fail "no count: $warning"
[ ! -e "$(file "$home_a")" ] && pass "the plain account is still done" || fail "a hostile name stopped the others"

# The postrm build-deb.sh writes: no temporary file, nothing on upgrade, and
# exit 0 with TMPDIR pointing nowhere.
for function_name in account_block write_activation_postrm; do
    definition="$(sed -n "/^$function_name() {/,/^}/p" "$ROOT_DIR/packaging/build-deb.sh")"
    [ -n "$definition" ] || {
        printf 'build-deb.sh has no %s\n' "$function_name" >&2
        exit 1
    }
    eval "$definition"
done
PACKAGE_ACTIVATION_NAME="$NAME"
PACKAGE_ACTIVATION_EXEC="$EXEC"
mkdir -p "$WORK_DIR/files/DEBIAN"
write_activation_postrm "$WORK_DIR/files"
postrm="$WORK_DIR/files/DEBIAN/postrm"
sh -n "$postrm" && pass "the postrm parses" || fail "the postrm does not parse"
! grep -q mktemp "$postrm" && pass "the postrm writes no temporary file" || fail "the postrm uses mktemp"
! grep -q 'better_os_passwd_file\|better_os_run_user_root' "$postrm" && pass "the postrm carries no copy of the account helpers" ||
    fail "the postrm still reads /etc/passwd itself"
sed -e "s|^better_os_login_defs=.*|better_os_login_defs='$WORK_DIR/login.defs'|" \
    -e "s|^better_os_getent=.*|better_os_getent='$WORK_DIR/getent'|" \
    -e "s|^better_os_session_root=.*|better_os_session_root='$WORK_DIR/run-user'|" \
    "$postrm" > "$WORK_DIR/postrm"
setup_homes
write_passwd
for action in upgrade failed-upgrade; do
    TMPDIR=/nonexistent sh "$WORK_DIR/postrm" "$action" 1.0 > /dev/null 2>&1 &&
        pass "postrm $action exits 0 with TMPDIR pointing nowhere" || fail "postrm $action failed"
done
[ -e "$(file "$home_a")" ] && pass "the generated postrm does nothing on upgrade" || fail "an upgrade removed the file"
for action in remove purge; do
    TMPDIR=/nonexistent sh "$WORK_DIR/postrm" "$action" 1.0 > /dev/null 2>&1 &&
        pass "postrm $action exits 0 with TMPDIR pointing nowhere" || fail "postrm $action failed"
done
[ ! -e "$(file "$home_a")" ] && [ -f "$(file "$home_g")" ] && [ ! -e "$(aside "$home_g")" ] &&
    pass "the generated postrm removes Better OS's file and puts the person's back" || fail "the generated postrm did not act"
exit "$FAILURES"
