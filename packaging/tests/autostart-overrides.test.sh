#!/usr/bin/env bash
# shellcheck disable=SC1091,SC2034 # the library is sourced with its header variables
# Drives packaging/maintainer/autostart-overrides.sh against a scratch passwd
# file and scratch homes. Nothing outside the scratch directory is read or
# written.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf -- "$WORK_DIR"' EXIT
FAILURES=0
fail() { printf 'FAIL: %s\n' "$1" >&2; FAILURES=$((FAILURES + 1)); }
pass() { printf 'ok: %s\n' "$1"; }

me="$(id -u)"
MARK='X-BetterOS-HiddenBy=better-touchpad'
home_a="$WORK_DIR/home/a"
home_b="$WORK_DIR/home/b"
mkdir -p "$home_a/.config/autostart" "$home_b/.config/autostart"
cat > "$WORK_DIR/passwd" <<PASSWD
root:x:0:0:root:/root:/bin/sh
a:x:$me:$me::$home_a:/bin/sh
b:x:$me:$me::$home_b:/bin/sh
svc:x:120:120::$WORK_DIR/home/svc:/usr/sbin/nologin
PASSWD

# Ours: removed. The person's own: kept. A link pointing at ours: kept.
printf '[Desktop Entry]\nHidden=true\n%s\n' "$MARK" > "$home_a/.config/autostart/touchegg.desktop"
printf '[Desktop Entry]\nExec=fusuma -d\n' > "$home_a/.config/autostart/fusuma.desktop"
printf '[Desktop Entry]\nHidden=true\n%s\n' "$MARK" > "$WORK_DIR/elsewhere.desktop"
ln -s "$WORK_DIR/elsewhere.desktop" "$home_b/.config/autostart/fusuma.desktop"
printf '[Desktop Entry]\nHidden=true\n# %s\n' "$MARK" > "$home_b/.config/autostart/touchegg.desktop"

run() {
    (
        better_os_passwd_file="$WORK_DIR/passwd"
        better_os_override_mark="$MARK"
        better_os_override_files='touchegg.desktop fusuma.desktop'
        . "$ROOT_DIR/packaging/maintainer/autostart-overrides.sh"
        better_os_forget_autostart_overrides "$@"
    )
}

run upgrade
[ -e "$home_a/.config/autostart/touchegg.desktop" ] && pass "an upgrade removes nothing" || fail "an upgrade removed an override"

if [ "$me" -lt 1000 ]; then
    printf 'skip: the test account (uid %s) is below 1000, the range the script reads\n' "$me"
    exit 0
fi

run remove
[ ! -e "$home_a/.config/autostart/touchegg.desktop" ] && pass "removal deletes Better OS's override" || fail "removal kept the override"
[ -e "$home_a/.config/autostart/fusuma.desktop" ] && pass "the person's own entry stays" || fail "the person's own entry was removed"
[ -L "$home_b/.config/autostart/fusuma.desktop" ] && [ -e "$WORK_DIR/elsewhere.desktop" ] && pass "a symbolic link is not followed or removed" || fail "a link was followed"
[ -e "$home_b/.config/autostart/touchegg.desktop" ] && pass "a marker that is not the exact line does not count" || fail "a commented marker was taken as ours"
run purge
pass "purge with nothing left is quiet"
exit "$FAILURES"
