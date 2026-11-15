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
home_c="$WORK_DIR/home/c" # ~/.config is a link
home_d="$WORK_DIR/home/d" # ~/.config/autostart is a link
mkdir -p "$home_a/.config/autostart" "$home_b/.config/autostart" "$home_c" "$home_d/.config"
mkdir -p "$WORK_DIR/linked-config/autostart" "$WORK_DIR/linked-autostart" "$WORK_DIR/rel/.config/autostart"
ln -s "$WORK_DIR/linked-config" "$home_c/.config"
ln -s "$WORK_DIR/linked-autostart" "$home_d/.config/autostart"
cat > "$WORK_DIR/passwd" <<PASSWD
root:x:0:0:root:/root:/bin/sh
a:x:$me:$me::$home_a:/bin/sh
b:x:$me:$me::$home_b:/bin/sh
c:x:$me:$me::$home_c:/bin/sh
d:x:$me:$me::$home_d:/bin/sh
rel:x:$me:$me::rel:/bin/sh
slash:x:$me:$me::/:/bin/sh
svc:x:120:120::$WORK_DIR/home/svc:/usr/sbin/nologin
PASSWD
# The scratch login range is exactly the test account, whatever its uid.
printf 'UID_MIN\t\t\t%s\nUID_MAX\t\t\t%s\n' "$me" "$me" > "$WORK_DIR/login.defs"
printf 'UID_MIN\t%s\nUID_MAX\t%s\n' "$((me + 1))" "$((me + 100))" > "$WORK_DIR/login.defs.above"

# Ours: removed. The person's own: kept. A link pointing at ours: kept.
printf '[Desktop Entry]\nHidden=true\n%s\n' "$MARK" > "$home_a/.config/autostart/touchegg.desktop"
printf '[Desktop Entry]\nExec=fusuma -d\n' > "$home_a/.config/autostart/fusuma.desktop"
printf '[Desktop Entry]\nHidden=true\n%s\n' "$MARK" > "$WORK_DIR/elsewhere.desktop"
ln -s "$WORK_DIR/elsewhere.desktop" "$home_b/.config/autostart/fusuma.desktop"
printf '[Desktop Entry]\nHidden=true\n# %s\n' "$MARK" > "$home_b/.config/autostart/touchegg.desktop"
ours() { printf '[Desktop Entry]\nHidden=true\n%s\n' "$MARK" > "$1"; }
ours "$WORK_DIR/linked-config/autostart/touchegg.desktop"
ours "$WORK_DIR/linked-autostart/touchegg.desktop"
ours "$WORK_DIR/rel/.config/autostart/touchegg.desktop"

run() {
    (
        cd "$WORK_DIR"
        better_os_passwd_file="$WORK_DIR/passwd"
        better_os_login_defs="${LOGIN_DEFS:-$WORK_DIR/login.defs}"
        better_os_override_mark="$MARK"
        better_os_override_files='touchegg.desktop fusuma.desktop'
        . "$ROOT_DIR/packaging/maintainer/autostart-overrides.sh"
        better_os_forget_autostart_overrides "$@"
    )
}

run upgrade
[ -e "$home_a/.config/autostart/touchegg.desktop" ] && pass "an upgrade removes nothing" || fail "an upgrade removed an override"

range() {
    (
        better_os_login_defs="$1"
        . "$ROOT_DIR/packaging/maintainer/autostart-overrides.sh"
        printf '%s..%s' "$(better_os_login_range_value UID_MIN 1000)" "$(better_os_login_range_value UID_MAX 60000)"
    )
}
[ "$(range "$WORK_DIR/missing")" = "1000..60000" ] && pass "no login.defs means 1000..60000, so nobody (65534) is skipped" || fail "default range is $(range "$WORK_DIR/missing")"
printf 'UID_MIN abc\nUID_MAX\n' > "$WORK_DIR/login.defs.bad"
[ "$(range "$WORK_DIR/login.defs.bad")" = "1000..60000" ] && pass "a login.defs value that is not a number falls back" || fail "bad range is $(range "$WORK_DIR/login.defs.bad")"

LOGIN_DEFS="$WORK_DIR/login.defs.above" run remove
[ -e "$home_a/.config/autostart/touchegg.desktop" ] && pass "an account outside UID_MIN..UID_MAX is skipped" || fail "an account outside the login range was touched"

run remove
[ ! -e "$home_a/.config/autostart/touchegg.desktop" ] && pass "removal deletes Better OS's override" || fail "removal kept the override"
[ -e "$home_a/.config/autostart/fusuma.desktop" ] && pass "the person's own entry stays" || fail "the person's own entry was removed"
[ -L "$home_b/.config/autostart/fusuma.desktop" ] && [ -e "$WORK_DIR/elsewhere.desktop" ] && pass "a symbolic link is not followed or removed" || fail "a link was followed"
[ -e "$home_b/.config/autostart/touchegg.desktop" ] && pass "a marker that is not the exact line does not count" || fail "a commented marker was taken as ours"
[ -e "$WORK_DIR/linked-config/autostart/touchegg.desktop" ] && pass "a symlinked ~/.config is not followed" || fail "a symlinked ~/.config was followed"
[ -e "$WORK_DIR/linked-autostart/touchegg.desktop" ] && pass "a symlinked ~/.config/autostart is not followed" || fail "a symlinked autostart was followed"
[ -e "$WORK_DIR/rel/.config/autostart/touchegg.desktop" ] && pass "a home that is not absolute is refused" || fail "a relative home was used"
run purge
pass "purge with nothing left is quiet"
exit "$FAILURES"
