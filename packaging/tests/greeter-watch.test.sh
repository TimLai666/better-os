#!/usr/bin/env bash
# The login screen watch's enablement against a fake deb-systemd-helper
# (ticket 140): enabled on a first install, kept enabled on an upgrade, and
# never re-enabled on an upgrade after an administrator disabled it.
set -euo pipefail
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
cat > "$work/bin/deb-systemd-helper" <<'FAKE'
#!/bin/sh
# State: $STATE/installed exists after a first enable; $STATE/enabled while
# enabled. Every call is logged.
echo "$*" >> "$STATE/log"
case "$1" in
    debian-installed) [ -e "$STATE/installed" ] ;;
    --quiet) [ "$2" = was-enabled ] && [ -e "$STATE/enabled" ] ;;
    enable) touch "$STATE/installed" "$STATE/enabled" ;;
    disable) rm -f "$STATE/enabled" ;;
    update-state) touch "$STATE/installed" ;;
esac
FAKE
chmod +x "$work/bin/deb-systemd-helper"
export PATH="$work/bin:$PATH" STATE="$work/state"

run() { sh -c ". '$ROOT_DIR/packaging/maintainer/greeter-watch.sh'; $1"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

mkdir -p "$STATE"
run 'better_os_greeter_watch_configure ""'
[ -e "$STATE/enabled" ] || fail "a first install did not enable the watch"

run 'better_os_greeter_watch_configure 0.2.16'
[ -e "$STATE/enabled" ] || fail "an upgrade turned an enabled watch off"

# An administrator disables it; the next upgrade keeps it disabled.
rm -f "$STATE/enabled"
: > "$STATE/log"
run 'better_os_greeter_watch_configure 0.2.17'
[ ! -e "$STATE/enabled" ] || fail "an upgrade re-enabled a watch an administrator disabled"
grep -q '^update-state better-greeter-watch.service$' "$STATE/log" || fail "no update-state: $(cat "$STATE/log")"
if grep -q '^enable ' "$STATE/log"; then fail "enable was called: $(cat "$STATE/log")"; fi

touch "$STATE/enabled"
run 'better_os_greeter_watch_disable'
[ ! -e "$STATE/enabled" ] || fail "removal left the watch enabled"
echo "greeter-watch maintainer script: ok"
