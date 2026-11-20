#!/usr/bin/env bash
# shellcheck disable=SC1091,SC2015,SC2030,SC2031,SC2034 # each case sources the library in a subshell with its header variables; checks read "test && pass || fail"
# Drives packaging/maintainer/files-activation.sh (ticket 129, ADR 0041)
# against a scratch passwd file and scratch homes. Nothing outside the
# scratch directory is read or written.
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
home_a="$WORK_DIR/home/a" # Better Defaults' file: removed
home_b="$WORK_DIR/home/b" # the person's own file: kept
home_c="$WORK_DIR/home/c" # ~/.local/share is a link: not followed
home_d="$WORK_DIR/home/d" # a link where the file would be: kept
home_e="$WORK_DIR/home/e" # Better Defaults' file with a sibling: dir kept
for home in "$home_a" "$home_b" "$home_d" "$home_e"; do
    mkdir -p "$home/.local/share/dbus-1/services"
done
mkdir -p "$home_c/.local" "$WORK_DIR/linked-share/dbus-1/services"
ln -s "$WORK_DIR/linked-share" "$home_c/.local/share"
cat > "$WORK_DIR/passwd" <<PASSWD
root:x:0:0:root:/root:/bin/sh
a:x:$me:$me::$home_a:/bin/sh
b:x:$me:$me::$home_b:/bin/sh
c:x:$me:$me::$home_c:/bin/sh
d:x:$me:$me::$home_d:/bin/sh
e:x:$me:$me::$home_e:/bin/sh
svc:x:120:120::$WORK_DIR/home/svc:/usr/sbin/nologin
PASSWD
printf 'UID_MIN\t%s\nUID_MAX\t%s\n' "$me" "$((me + 1))" > "$WORK_DIR/login.defs"

ours() { printf '[D-BUS Service]\nName=%s\nExec=%s\n' "$NAME" "$EXEC" > "$1"; }
file() { printf '%s/.local/share/dbus-1/services/%s.service' "$1" "$NAME"; }
ours "$(file "$home_a")"
printf '[D-BUS Service]\nName=%s\nExec=/opt/other --dbus\n' "$NAME" > "$(file "$home_b")"
ours "$WORK_DIR/linked-share/dbus-1/services/$NAME.service"
ours "$WORK_DIR/elsewhere.service"
ln -s "$WORK_DIR/elsewhere.service" "$(file "$home_d")"
ours "$(file "$home_e")"
printf 'x\n' > "$home_e/.local/share/dbus-1/services/other.service"

run() {
    (
        cd "$WORK_DIR"
        better_os_passwd_file="$WORK_DIR/passwd"
        better_os_login_defs="$WORK_DIR/login.defs"
        better_os_override_mark=''
        better_os_override_files=''
        better_os_run_user_root="$WORK_DIR/run/user"
        better_os_activation_name="$NAME"
        better_os_activation_exec="$EXEC"
        . "$ROOT_DIR/packaging/maintainer/autostart-overrides.sh"
        . "$ROOT_DIR/packaging/maintainer/files-activation.sh"
        better_os_forget_activation_files "$@"
    )
}

run upgrade
[ -e "$(file "$home_a")" ] && pass "an upgrade removes nothing" || fail "an upgrade removed the file"

run remove
[ ! -e "$(file "$home_a")" ] && pass "removal deletes the file Better Defaults wrote" || fail "the file stayed"
[ ! -e "$home_a/.local/share/dbus-1/services" ] && pass "the directory it left empty goes too" || fail "an empty services directory stayed"
[ -e "$(file "$home_b")" ] && pass "the person's own file stays" || fail "the person's own file was removed"
[ -e "$WORK_DIR/linked-share/dbus-1/services/$NAME.service" ] && pass "a symlinked ~/.local/share is not followed" || fail "a linked directory was followed"
[ -L "$(file "$home_d")" ] && [ -e "$WORK_DIR/elsewhere.service" ] && pass "a symbolic link is not followed or removed" || fail "a link was followed"
[ ! -e "$(file "$home_e")" ] && [ -e "$home_e/.local/share/dbus-1/services/other.service" ] && pass "a directory with other files keeps them" || fail "other services were touched"
run purge
pass "purge with nothing left is quiet"
exit "$FAILURES"
