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
home_e="$WORK_DIR/home/e" # another account: reached only through setpriv
mkdir -p "$home_a/.config/autostart" "$home_b/.config/autostart" "$home_c" "$home_d/.config" "$home_e/.config/autostart"
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
e:x:$((me + 1)):$((me + 1))::$home_e:/bin/sh
svc:x:120:120::$WORK_DIR/home/svc:/usr/sbin/nologin
PASSWD
# The scratch login range is exactly the test account, whatever its uid.
printf 'UID_MIN\t\t\t%s\nUID_MAX\t\t\t%s\n' "$me" "$((me + 1))" > "$WORK_DIR/login.defs"
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
# A temporary copy the page left when it crashed mid-write: ours, removed.
ours "$home_a/.config/autostart/fusuma.desktop.tmp-better-os"
# Account e's override. Its files are ours on disk, so the stat says "not
# owned by e" and nothing is touched; the stand-in setpriv records that it
# would only ever have been asked with e's ids.
ours "$home_e/.config/autostart/touchegg.desktop"
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
        cd "$WORK_DIR"
        better_os_passwd_file="$WORK_DIR/passwd"
        better_os_login_defs="${LOGIN_DEFS:-$WORK_DIR/login.defs}"
        better_os_override_mark="$MARK"
        better_os_override_files='touchegg.desktop fusuma.desktop'
        better_os_setpriv="$WORK_DIR/setpriv"
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
[ ! -e "$home_a/.config/autostart/fusuma.desktop.tmp-better-os" ] && pass "a crashed write's temporary copy is removed" || fail "the temporary copy stayed"
[ -e "$home_e/.config/autostart/touchegg.desktop" ] && pass "a file not owned by its account is left" || fail "another owner's file was removed"
[ ! -e "$WORK_DIR/setpriv.log" ] && pass "this account's own files need no setpriv" || fail "setpriv ran for the test's own account"
# The account switch itself: e's directories and file owned by e are not
# possible without root, so the helper is called directly.
(
    better_os_setpriv="$WORK_DIR/setpriv"
    . "$ROOT_DIR/packaging/maintainer/autostart-overrides.sh"
    better_os_as_account "$((me + 1))" "$((me + 1))" true
)
grep -qx -- "--reuid=$((me + 1)) --regid=$((me + 1)) --init-groups -- true" "$WORK_DIR/setpriv.log" && pass "another account's delete runs as that account" || fail "the delete did not switch accounts: $(cat "$WORK_DIR/setpriv.log" 2>/dev/null)"
run purge
pass "purge with nothing left is quiet"
exit "$FAILURES"
