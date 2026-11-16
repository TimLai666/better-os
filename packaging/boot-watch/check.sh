#!/usr/bin/env bash
# The login screen watch against a real systemd, logind, and journal, with a
# stand-in gdm.service that never opens a session (ADR 0033, ticket 140).
# Runs only inside the container packaging/boot-watch/run.sh starts.
set -euo pipefail
[[ "${BETTER_OS_E2E_CONTAINER:-}" == 1 ]] || {
    echo "check.sh restarts gdm.service; run it through packaging/boot-watch/run.sh" >&2
    exit 2
}
dir=/var/lib/better-os/boot-watch
fail() { echo "FAIL: $*" >&2; exit 1; }
pass() { echo "ok: $*"; }
invocation() { systemctl show -p InvocationID --value gdm.service; }

systemctl daemon-reload
systemctl enable --now gdm.service
systemctl enable better-greeter-watch.service
[[ "$(systemctl show -p Id --value display-manager.service)" == gdm.service ]] \
    || fail "display-manager.service does not resolve to the stand-in"

# Off the boot's critical path: wanting the unit does not order the target
# after it.
systemctl show -p Wants --value graphical.target | grep -qw better-greeter-watch.service \
    || fail "graphical.target does not want the watch"
if systemctl show -p After --value graphical.target | grep -qw better-greeter-watch.service; then
    fail "graphical.target is ordered after the watch"
fi
pass "graphical.target wants the watch and does not wait for it"

# Turned off: nothing is restarted.
mkdir -p "$dir"
printf '{"enabled":false}\n' > "$dir/settings.json"
before="$(invocation)"
systemctl start better-greeter-watch.service
grep -q '"outcome": "turned_off"' "$dir/last-boot.json" || fail "turned off: $(cat "$dir/last-boot.json")"
[[ "$(invocation)" == "$before" ]] || fail "turned off and gdm was restarted"
pass "turned off: gdm left alone"
rm "$dir/settings.json"
# The check stands in for a second boot: this one is now recorded as watched.
rm "$dir/last-boot.json"

# The stand-in never opens a session: evidence, one restart, the outcome.
before="$(invocation)"
started=$SECONDS
systemctl restart better-greeter-watch.service
took=$((SECONDS - started))
after="$(invocation)"
[[ "$after" != "$before" ]] || fail "gdm was not restarted"
grep -q '"outcome": "restarted"' "$dir/last-boot.json" || fail "last boot: $(cat "$dir/last-boot.json")"
shopt -s nullglob
events=("$dir"/evidence/[0-9]*-*.json)
[[ ${#events[@]} -eq 1 ]] || fail "expected one event record, found ${#events[@]}"
event="${events[0]}"
[[ "$(stat -c %a:%U:%G "$event")" == 640:root:adm ]] || fail "the record is $(stat -c %a:%U:%G "$event")"
[[ "$(stat -c %a:%U:%G "$dir/evidence")" == 750:root:adm ]] || fail "the evidence directory is $(stat -c %a:%U:%G "$dir/evidence")"
[[ "$(stat -c %a "$dir")" == 755 ]] || fail "the records directory is not 0755"
[[ "$(stat -c %a "$dir/summary.json")" == 644 ]] || fail "the summary is not 0644"
grep -q 'Stand-in GDM' "$dir/summary.json" && fail "the public summary carries journal lines"
[[ "$(systemctl show -p ActiveState --value better-greeter-watch.service)" == active ]] \
    || fail "RemainAfterExit: the unit is not active after its run"
# Someone outside adm reads the summary and not the journal excerpts.
setpriv --reuid=nobody --regid=nogroup --clear-groups cat "$dir/summary.json" >/dev/null \
    || fail "nobody cannot read the summary"
if setpriv --reuid=nobody --regid=nogroup --clear-groups cat "$event" >/dev/null 2>&1; then
    fail "nobody can read the evidence"
fi
grep -q '"result": "still_missing"' "$event" || fail "outcome: $(cat "$event")"
grep -q '"display_manager": "gdm.service"' "$event" || fail "unit not recorded"
grep -q 'Stand-in GDM' "$event" || fail "GDM's journal is not in the evidence"
grep -q '"waited_seconds": 3[0-9]' "$event" || fail "did not wait for the deadline"
(( took < 180 )) || fail "the watch took ${took}s"
pass "late login screen: recorded, gdm restarted once, outcome still_missing (${took}s)"

# The same boot again, even forced: never a second restart.
before="$(invocation)"
systemctl restart better-greeter-watch.service
grep -q '"outcome": "already_handled"' "$dir/last-boot.json" || fail "second run: $(cat "$dir/last-boot.json")"
[[ "$(invocation)" == "$before" ]] || fail "gdm was restarted twice on one boot"
events=("$dir"/evidence/[0-9]*-*.json)
[[ ${#events[@]} -eq 1 ]] || fail "a second record was written"
pass "second run on the same boot: nothing restarted"

# No GDM: the watch does nothing and says so.
systemctl disable --now gdm.service
rm -f /etc/systemd/system/display-manager.service
systemctl daemon-reload
rm -f "$dir"/evidence/[0-9]*-*.json "$dir/last-boot.json"
systemctl restart better-greeter-watch.service
grep -q '"outcome": "no_gdm"' "$dir/last-boot.json" || fail "no gdm: $(cat "$dir/last-boot.json")"
pass "no display manager: nothing done, reason recorded"
echo "boot-watch check passed"
