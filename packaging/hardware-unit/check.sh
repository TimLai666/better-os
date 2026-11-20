#!/usr/bin/env bash
# Runs inside the disposable container or VM `run.sh` (or a cloud-init seed)
# starts, never on a machine anyone uses. It covers the places the checks
# write with empty tmpfs stand-ins first and stops before any write if a cover
# is missing. ADR 0029: the daemon and the restore cannot write /sys; only the
# transient helper can, and only its one bound file.
set -euo pipefail

if [[ "${BETTER_OS_E2E_CONTAINER:-}" != "1" ]]; then
    echo "Refusing to run outside the disposable container (BETTER_OS_E2E_CONTAINER=1)." >&2
    exit 2
fi

# sysfs takes no mkdir, so the covers go on directories every kernel has.
for cover in /sys/devices/platform /sys/class; do
    [[ -d "$cover" ]] || { echo "FAIL: $cover is missing in this kernel" >&2; exit 1; }
    mount -t tmpfs -o size=1m,mode=0755 better-os-standin "$cover"
    if [[ "$(findmnt -n -o SOURCE --mountpoint "$cover")" != "better-os-standin" ]]; then
        echo "FAIL: $cover is not covered by the stand-in; nothing was written" >&2
        exit 1
    fi
done
platform=/sys/devices/platform/asus-nb-wmi
chip="$platform/hwmon/hwmon8"
bat=/sys/devices/platform/standin-BAT1
mkdir -p "$chip" "$bat" /sys/class/power_supply /sys/class/hwmon
echo asus > "$chip/name"
echo 2 > "$chip/pwm1_enable"
ln -s "$platform" "$chip/device"
ln -s "$chip" /sys/class/hwmon/hwmon8
echo Battery > "$bat/type"
echo 87 > "$bat/capacity"
echo 100 > "$bat/charge_control_end_threshold"
ln -s "$bat" /sys/class/power_supply/BAT1
# A real kernel file, written back with the value it already has.
real=/sys/kernel/mm/transparent_hugepage/enabled
real_value="$(sed -n 's/.*\[\(.*\)\].*/\1/p' "$real")"
fail() { echo "FAIL: $*"; exit 1; }

systemctl daemon-reload

# 1. The real restore unit: the stored 80 reaches the battery through the
#    helper, from a unit whose /sys is read-only.
mkdir -p /var/lib/better-os
echo '{"charge_limit":80}' > /var/lib/better-os/hardware.json
systemctl start better-hardware-restore.service || true
journalctl -u better-hardware-restore.service -o cat --no-pager | tail -n 2
[[ "$(cat "$bat/charge_control_end_threshold")" == "80" ]] || fail "the restore wrote nothing"
echo "PASS restore unit: charge limit 100 -> 80 through the helper"

# 2. Units with the daemon's own [Service] sandbox.
service_lines="$(sed -n '/^\[Service\]/,$p' /usr/lib/systemd/system/better-manager-daemon.service \
    | grep -vE '^\[|^(Type|BusName|ExecStart|Restart)=|^#|^$')"
sandboxed() {
    local name="$1"; shift
    {
        echo "[Service]"
        echo "Type=oneshot"
        printf 'ExecStart=%s\n' "$*"
        printf '%s\n' "$service_lines"
    } > "/run/systemd/system/$name.service"
    systemctl daemon-reload
    timeout 120 systemctl start "$name.service" || true
}
sandboxed t86-daemon-fan /bin/sh -c "'echo 0 > $chip/pwm1_enable'"
[[ "$(cat "$chip/pwm1_enable")" == "2" ]] || fail "the daemon's sandbox wrote the fan switch itself"
echo "PASS daemon sandbox: cannot write the fan switch itself"
sandboxed t86-daemon-real /bin/sh -c "'echo $real_value > $real && touch /run/t86-real-written'"
[[ ! -e /run/t86-real-written ]] || fail "the daemon's sandbox wrote $real"
echo "PASS daemon sandbox: cannot write $real"
swap="$(cat /proc/sys/vm/swappiness)"
sandboxed t86-daemon-proc /bin/sh -c "'echo $swap > /proc/sys/vm/swappiness && touch /run/t86-proc-written'"
[[ ! -e /run/t86-proc-written ]] || fail "the daemon's sandbox wrote /proc/sys"
echo "PASS daemon sandbox: cannot write /proc/sys"
sandboxed t86-daemon-direct /usr/libexec/better-manager-daemon --write-attribute "$chip/pwm1_enable" 0
[[ "$(cat "$chip/pwm1_enable")" == "2" ]] || fail "the helper code ran unsandboxed and wrote"
echo "PASS daemon sandbox: the helper's own code cannot write from there either"
sandboxed t86-daemon-helper /usr/libexec/better-manager-daemon --write-via-helper "$chip/pwm1_enable" 0
if [[ "$(cat "$chip/pwm1_enable")" != "0" ]]; then
    journalctl -u t86-daemon-helper.service -o cat --no-pager | tail -n 15
    fail "the helper started from the daemon's sandbox did not write"
fi
echo "PASS daemon sandbox -> helper: pwm1_enable 2 -> 0"

# The restore unit's own sandbox cannot write either.
service_lines="$(sed -n '/^\[Service\]/,/^\[Install\]/p' /usr/lib/systemd/system/better-hardware-restore.service \
    | grep -vE '^\[|^(Type|ExecStart)=|^#|^$')"
echo 100 > "$bat/charge_control_end_threshold"
sandboxed t86-restore-direct /bin/sh -c "'echo 60 > $bat/charge_control_end_threshold; echo $real_value > $real && touch /run/t86-restore-real'"
[[ "$(cat "$bat/charge_control_end_threshold")" == "100" && ! -e /run/t86-restore-real ]] \
    || fail "the restore unit's sandbox wrote /sys itself"
echo "PASS restore sandbox: cannot write /sys itself"

# 3. The helper itself: its one file and nothing else. These are exactly
# manager_daemon::hardware::helper_sandbox() and the attribute helper's own
# two lines (ADR 0029, narrowed by ADR 0034's review); keep them in step.
props=(-p TemporaryFileSystem=/sys:ro -p "BindPaths=$chip/pwm1_enable" -p MountAPIVFS=no
       -p ProtectSystem=strict -p ProtectHome=yes -p PrivateTmp=yes -p PrivateNetwork=yes
       -p PrivateDevices=yes -p ReadOnlyPaths=/proc/sys -p ReadOnlyPaths=/proc/sysrq-trigger
       -p ProtectKernelModules=yes -p ProtectKernelLogs=yes -p ProtectClock=yes
       -p ProtectHostname=yes -p NoNewPrivileges=yes -p RestrictSUIDSGID=yes
       -p RestrictRealtime=yes -p RestrictAddressFamilies=AF_UNIX -p RestrictNamespaces=yes
       -p LockPersonality=yes -p MemoryDenyWriteExecute=yes -p SystemCallArchitectures=native
       -p SystemCallFilter=@system-service -p RuntimeMaxSec=10s -p CapabilityBoundingSet=)
out="$(systemd-run --quiet --wait --pipe --collect "${props[@]}" /bin/sh -c \
    "echo $real_value > $real && echo WROTE-REAL; echo 1 > $bat/charge_control_end_threshold && echo WROTE-OTHER; touch /sys/new && echo CREATED; echo '|/bin/false' > /proc/sys/kernel/core_pattern && echo WROTE-CORE; echo /bin/false > /proc/sys/kernel/modprobe && echo WROTE-MODPROBE; mknod /dev/better-os-probe c 1 3 && echo MADE-NODE; touch /dev/better-os-probe-file && echo MADE-FILE; [ \"\$(ls /sys | wc -l)\" -gt 1 ] && echo WHOLE-SYS; true" 2>/dev/null || true)"
for marker in WROTE-REAL WROTE-OTHER CREATED WROTE-CORE WROTE-MODPROBE MADE-NODE MADE-FILE WHOLE-SYS; do
    [[ "$out" != *"$marker"* ]] || fail "the helper sandbox let through $marker: $out"
done
echo "PASS helper sandbox: no other sysfs file or attribute, nothing new under /sys or /dev, /proc/sys read-only, /sys is one file"
# The same sandbox still writes the one file it binds (no property above
# takes the bound file away).
out="$(systemd-run --quiet --wait --pipe --collect "${props[@]}" /bin/sh -c "echo 2 > $chip/pwm1_enable && echo WROTE-OWN" 2>&1 || true)"
[[ "$out" == *WROTE-OWN* && "$(cat "$chip/pwm1_enable")" == "2" ]] || fail "the hardened helper could not write its own file: $out"
echo "PASS helper sandbox: its own file is still written"
out="$(systemd-run --quiet --wait --pipe --collect /usr/libexec/better-manager-daemon --write-via-helper "$real" "$real_value" 2>&1 || true)"
[[ "$out" == *invalid_argument* ]] || fail "the helper accepted $real: $out"
echo "PASS helper refuses any file but the two attributes"
systemctl --version | head -n 1
