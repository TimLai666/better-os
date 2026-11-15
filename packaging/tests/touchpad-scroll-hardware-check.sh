#!/usr/bin/env bash
# The owner's real-hardware check for the scroll pass-through (ticket 125,
# ADR 0030). Run it on the machine with the touchpad, after installing the
# better-touchpad package built from this branch, as yourself (not root):
#
#   bash packaging/tests/touchpad-scroll-hardware-check.sh
#
# It changes the scroll factors through the same bus call the window makes,
# asks you to scroll, and puts the factors back to what they were. It never
# opens a device itself. Every step prints what it saw; a FAIL line names
# the claim in engineering/specs/touchpad-input-passthrough-security.md that
# did not hold.
set -uo pipefail

fail() { printf 'FAIL: %s\n' "$1"; FAILED=1; }
ok() { printf 'ok: %s\n' "$1"; }
FAILED=0

mapfile -t eligible < <(/usr/libexec/better-touchpad-scrolld --eligible)
((${#eligible[@]} > 0)) || { fail "no touchpad is eligible"; exit 1; }
ok "eligible touchpads: ${eligible[*]}"

for event in "${eligible[@]}"; do
    unit="better-touchpad-scroll@$event.service"
    systemctl is-active --quiet "$unit" && ok "$unit is running" || fail "$unit is not running after install"
    allow="$(systemctl show -p DeviceAllow --value "$unit")"
    [[ "$allow" == *"/dev/input/$event r"* && "$allow" == *"/dev/uinput w"* ]] \
        && ok "$unit may open only its node and uinput: $allow" \
        || fail "DeviceAllow was not expanded as expected: $allow"
    user="$(ps -o user= -p "$(systemctl show -p MainPID --value "$unit")")"
    [[ "$user" == _better-touch* ]] && ok "$unit runs as $user" || fail "$unit runs as $user"
done

# No instance may run for a node that is not a touchpad.
while read -r unit; do
    event="${unit#better-touchpad-scroll@}"; event="${event%.service}"
    printf '%s\n' "${eligible[@]}" | grep -qx "$event" || fail "an instance runs for $event, which is not eligible"
done < <(systemctl list-units --plain --no-legend 'better-touchpad-scroll@*' | awk '{print $1}')

name="org.betteros.TouchpadScroll1.${eligible[0]}"
before="$(busctl --system call "$name" /org/betteros/TouchpadScroll1 org.betteros.TouchpadScroll1 Describe)"
ok "in force before: $before"
vertical="$(awk '{print $(NF-1)}' <<< "$before")"
horizontal="$(awk '{print $NF}' <<< "$before")"

busctl --system call "$name" /org/betteros/TouchpadScroll1 org.betteros.TouchpadScroll1 SetFactors dd 3 1 \
    && ok "vertical 3 set" || fail "SetFactors was refused for the person at the machine"
read -r -p "Scroll a long page with two fingers. Does it move about three times as far? [y/n] " answer
[[ "$answer" == y ]] && ok "vertical scroll is scaled" || fail "vertical scroll did not change"
read -r -p "Move the pointer with one finger, tap, and three-finger swipe. All as before? [y/n] " answer
[[ "$answer" == y ]] && ok "pointer, tap, and gestures are unchanged" || fail "something besides scrolling changed"
read -r -p "Type in a text field while touching the pad. Does disable-while-typing still work? [y/n] " answer
[[ "$answer" == y ]] && ok "disable-while-typing still pairs" || fail "disable-while-typing lost its pairing"

# A hang must give the touchpad back: stop the process, wait for the watchdog.
pid="$(systemctl show -p MainPID --value "better-touchpad-scroll@${eligible[0]}.service")"
printf 'Freezing the service (SIGSTOP) as a hang; the watchdog should end it within about 10 s.\n'
sudo kill -STOP "$pid"
sleep 12
read -r -p "Is the touchpad working again (unscaled, then scaled after the restart)? [y/n] " answer
[[ "$answer" == y ]] && ok "a hang gives the touchpad back" || fail "the touchpad stayed held after a hang"

busctl --system call "$name" /org/betteros/TouchpadScroll1 org.betteros.TouchpadScroll1 SetFactors dd \
    "$vertical" "$horizontal" && ok "factors put back to $vertical $horizontal"

printf '\nAdded cost per read on this machine (logged once a minute while holding):\n'
journalctl -b -u 'better-touchpad-scroll@*' --no-pager | grep 'added' | tail -5
journalctl -b -u 'better-touchpad-scroll@*' --no-pager | grep -q 'is not held' \
    && journalctl -b -u 'better-touchpad-scroll@*' --no-pager | grep 'is not held'
exit "$FAILED"
