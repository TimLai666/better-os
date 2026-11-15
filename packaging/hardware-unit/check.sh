#!/usr/bin/env bash
# Runs inside the container `run.sh` starts. It covers the two places the
# daemon writes with empty tmpfs stand-ins first, so nothing here can reach
# the real machine's sysfs, and stops before any write if a cover is missing.
set -euo pipefail

if [[ "${BETTER_OS_E2E_CONTAINER:-}" != "1" ]]; then
    echo "Refusing to run outside the disposable container (BETTER_OS_E2E_CONTAINER=1)." >&2
    exit 2
fi

# sysfs takes no mkdir, so the covers go on directories every kernel has:
# all platform devices, and every device class (a VM's kernel may have no
# power_supply class at all).
platform=/sys/devices/platform/asus-nb-wmi
class=/sys/class/power_supply
for cover in /sys/devices/platform /sys/class; do
    [[ -d "$cover" ]] || { echo "FAIL: $cover is missing in this kernel" >&2; exit 1; }
    mount -t tmpfs -o size=1m,mode=0755 better-os-standin "$cover"
    if [[ "$(findmnt -n -o FSTYPE --mountpoint "$cover")" != "tmpfs" ]] \
        || [[ "$(findmnt -n -o SOURCE --mountpoint "$cover")" != "better-os-standin" ]]; then
        echo "FAIL: $cover is not covered by the stand-in; nothing was written" >&2
        exit 1
    fi
done
mkdir -p "$platform" "$class"

# The stand-ins: an asus-nb-wmi fan switch, and a battery whose class link
# points into the covered platform directory, as a real class link points at
# its device.
chip="$platform/hwmon/hwmon8"
mkdir -p "$chip" "$platform/standin-BAT1"
echo asus > "$chip/name"
echo 2 > "$chip/pwm1_enable"
ln -s "$platform" "$chip/device"
bat="$platform/standin-BAT1"
echo Battery > "$bat/type"
echo 87 > "$bat/capacity"
echo 100 > "$bat/charge_control_end_threshold"
ln -s "$bat" "$class/BAT1"

systemctl daemon-reload

# 1. The real restore unit and the real daemon program: the stored 80 is
#    written through ReadWritePaths=-/sys/class/power_supply/BAT1.
mkdir -p /var/lib/better-os
echo '{"charge_limit":80}' > /var/lib/better-os/hardware.json
systemctl start better-hardware-restore.service || true
systemctl status --no-pager better-hardware-restore.service | tail -n 3 || true
got="$(cat "$bat/charge_control_end_threshold")"
[[ "$got" == "80" ]] || { echo "FAIL: restore unit wrote nothing (reads $got)"; exit 1; }
echo "PASS restore unit: charge limit 100 -> 80 under ProtectKernelTunables=yes"

# 2. The daemon unit's own [Service] section, with a write in place of the
#    daemon, through ReadWritePaths=-/sys/devices/platform/asus-nb-wmi and a
#    class link whose hwmonN is below it.
service_lines="$(sed -n '/^\[Service\]/,/^\[/p' /usr/lib/systemd/system/better-manager-daemon.service \
    | grep -vE '^\[|^(Type|BusName|ExecStart|Restart)=')"
write_unit() {
    local name="$1" body="$2" extra="$3"
    {
        echo "[Service]"
        echo "Type=oneshot"
        echo "ExecStart=/bin/sh -c '$body'"
        printf '%s\n' "$service_lines" | grep -vE "$extra"
    } > "/run/systemd/system/$name.service"
}
write_unit better-fan-write 'echo 0 > /sys/devices/platform/asus-nb-wmi/hwmon/hwmon8/pwm1_enable' '^$'
# 3. The negative control: the same unit without its ReadWritePaths= lines.
write_unit better-fan-control 'echo 0 > /sys/devices/platform/asus-nb-wmi/hwmon/hwmon8/pwm1_enable' '^ReadWritePaths='
# 4. Something else under /sys stays read-only even with the lines.
write_unit better-other-write 'echo 1 > /sys/class/power_supply/standin-elsewhere' '^$'
systemctl daemon-reload

systemctl start better-fan-write.service || true
[[ "$(cat "$chip/pwm1_enable")" == "0" ]] \
    || { echo "FAIL: the daemon unit's ReadWritePaths did not open the fan switch"; exit 1; }
echo "PASS daemon unit: pwm1_enable 2 -> 0 under ProtectKernelTunables=yes"

# What ProtectKernelTunables=yes does to the rest of /sys on this systemd.
# Not a pass or a fail: on Ubuntu 24.04's systemd 255 it leaves /sys
# writable (ticket 86 records this; see its Notes), so these say which.
echo 2 > "$chip/pwm1_enable"
systemctl start better-fan-control.service || true
if [[ "$(cat "$chip/pwm1_enable")" == "2" ]]; then
    echo "OBSERVED: without ReadWritePaths the attribute is read-only"
else
    echo "OBSERVED: without ReadWritePaths the attribute is still writable (/sys is not read-only)"
fi
systemctl start better-other-write.service || true
if [[ -e "$class/standin-elsewhere" ]]; then
    echo "OBSERVED: a path outside ReadWritePaths is writable"
else
    echo "OBSERVED: a path outside ReadWritePaths is read-only"
fi
systemctl --version | head -n 1
