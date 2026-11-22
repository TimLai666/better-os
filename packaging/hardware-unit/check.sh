#!/usr/bin/env bash
# Runs inside the disposable container or VM `run.sh` (or a cloud-init seed)
# starts, never on a machine anyone uses. It covers the places the checks
# write with empty tmpfs stand-ins first and stops before any write if a cover
# is missing. ADR 0029: the daemon and the restore cannot write /sys; only the
# transient helper can, and only its one bound file. ADR 0046: the USB-C
# recovery's two helpers and its after-sleep unit start with their own lists.
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

# 3. The helper itself: its one file and nothing else. The properties are
# the daemon's own (`--helper-properties`, the list SandboxedWriter passes to
# systemd-run), so this check cannot drift from the code (ADR 0029, narrowed
# by ADR 0034's review).
props_for() {
    local line
    probe_props=()
    while IFS= read -r line; do
        [[ -n "$line" ]] && probe_props+=(-p "$line")
    done < <(/usr/libexec/better-manager-daemon --helper-properties "$@")
    [[ ${#probe_props[@]} -gt 0 ]] || fail "the daemon printed no helper properties for $*"
}
props_for attribute "$chip/pwm1_enable"
printf '%s\n' "${probe_props[@]}" | grep -qx 'PrivateDevices=yes' || fail "the attribute helper's list has no private /dev"
printf '%s\n' "${probe_props[@]}" | grep -qx 'RuntimeMaxSec=10s' || fail "the attribute helper's list has no time limit"
if printf '%s\n' "${probe_props[@]}" | grep -qE '^(ProtectKernelTunables|ProtectControlGroups|ProtectProc|PrivateNetwork)='; then
    fail "the attribute helper's list brings the API file systems back"
fi
# The unit starts with every property (systemd-run refuses an unknown or
# malformed one before anything runs) and the probe sees one file under /sys.
probe='echo PROBE-STARTED'
probe="$probe; [ \"\$(ls -A /sys | wc -l)\" -eq 1 ] && echo SYS-ONE-ENTRY"
probe="$probe; echo $real_value > $real && echo WROTE-REAL"
probe="$probe; echo 1 > $bat/charge_control_end_threshold && echo WROTE-OTHER"
probe="$probe; touch /sys/new && echo CREATED"
probe="$probe; grep -q ' /proc/sys ro,' /proc/self/mountinfo && echo PROC-SYS-RO"
probe="$probe; echo '|/bin/false' > /proc/sys/kernel/core_pattern && echo WROTE-CORE"
probe="$probe; echo /bin/false > /proc/sys/kernel/modprobe && echo WROTE-MODPROBE"
probe="$probe; mknod /dev/better-os-probe c 1 3 && echo MADE-NODE"
probe="$probe; touch /dev/better-os-probe-file && echo MADE-FILE"
probe="$probe; [ -e /dev/sda ] || [ -e /dev/vda ] || [ -e /dev/kmsg ] || [ -e /dev/mem ] && echo HOST-DEV"
probe="$probe; true"
out="$(systemd-run --quiet --wait --pipe --collect "${probe_props[@]}" /bin/sh -c "$probe" 2>&1 || true)"
for wanted in PROBE-STARTED SYS-ONE-ENTRY PROC-SYS-RO; do
    [[ "$out" == *"$wanted"* ]] || fail "the attribute helper's unit did not show $wanted: $out"
done
for marker in WROTE-REAL WROTE-OTHER CREATED WROTE-CORE WROTE-MODPROBE MADE-NODE MADE-FILE HOST-DEV; do
    [[ "$out" != *"$marker"* ]] || fail "the helper sandbox let through $marker: $out"
done
echo "PASS helper sandbox: started with the daemon's list; /sys is one file, /proc/sys read-only, /dev private, nothing else written"
# The same unit still writes the one file it binds.
out="$(systemd-run --quiet --wait --pipe --collect "${probe_props[@]}" /bin/sh -c "echo 2 > $chip/pwm1_enable && echo WROTE-OWN" 2>&1 || true)"
[[ "$out" == *WROTE-OWN* && "$(cat "$chip/pwm1_enable")" == "2" ]] || fail "the hardened helper could not write its own file: $out"
echo "PASS helper sandbox: its own file is still written"
# And the real helper, started as the daemon starts it, writes through it.
/usr/libexec/better-manager-daemon --write-via-helper "$chip/pwm1_enable" 0 \
    || fail "the helper started from a root shell did not write"
[[ "$(cat "$chip/pwm1_enable")" == "0" ]] || fail "the helper's read back is not what it wrote"
echo 2 > "$chip/pwm1_enable"
echo "PASS helper: pwm1_enable 2 -> 0 with the full property list"
out="$(systemd-run --quiet --wait --pipe --collect /usr/libexec/better-manager-daemon --write-via-helper "$real" "$real_value" 2>&1 || true)"
[[ "$out" == *invalid_argument* ]] || fail "the helper accepted $real: $out"
echo "PASS helper refuses any file but the attributes it knows"

# 4. The LampArray helper (ADR 0034). This container has no hidraw device,
# and none can be made that the kernel's hidraw driver answers for: what is
# checked is that systemd accepts the unit's properties and that the helper
# fails cleanly, never that a keyboard is driven (ADR 0034, not verified).
cursor="$(journalctl -n 0 --show-cursor -q --no-pager | sed -n 's/^-- cursor: //p')"
lamp_failed_cleanly() {
    # The transient unit ran the helper, which exited 1: systemd accepted
    # every property, and the failure is the helper's own.
    local after="$1"
    journalctl --after-cursor="$after" --no-pager -o cat \
        | grep -E "Main process exited, code=exited, status=1/FAILURE" >/dev/null
}
# 4a. A node that does not exist.
status=0
out="$(/usr/libexec/better-manager-daemon --lamp-array-via-helper hidraw97 describe 2>&1)" || status=$?
[[ "$status" -ne 0 ]] || fail "describing an absent node succeeded: $out"
[[ "$out" == *"/dev/hidraw97"* && "$out" == *"No such file or directory"* ]] \
    || fail "an absent node did not fail as the helper's own No such file: $out"
[[ "$out" != *"Unknown assignment"* && "$out" != *"Failed to start transient"* ]] \
    || fail "systemd refused the LampArray helper's properties: $out"
lamp_failed_cleanly "$cursor" || {
    journalctl --after-cursor="$cursor" --no-pager -o cat | tail -n 20
    fail "the LampArray helper's unit did not run and exit 1 for an absent node"
}
echo "PASS lamp helper: properties accepted; an absent node fails cleanly (exit 1, No such file)"
# 4b. A character device that is not hidraw, bound into the private /dev and
# allowed by the device cgroup: the open must not be refused by the cgroup
# (EPERM), and the helper must refuse the node for not answering hidraw's
# ioctl or for having no driver. Major 10 minor 229 is /dev/fuse.
mknod /dev/hidraw98 c 10 229
cursor="$(journalctl -n 0 --show-cursor -q --no-pager | sed -n 's/^-- cursor: //p')"
status=0
out="$(/usr/libexec/better-manager-daemon --lamp-array-via-helper hidraw98 describe 2>&1)" || status=$?
rm -f /dev/hidraw98
[[ "$status" -ne 0 ]] || fail "a node that is not hidraw was described: $out"
[[ "$out" == *"/dev/hidraw98"* ]] || fail "the helper's refusal does not name the node: $out"
[[ "$out" != *"Operation not permitted"* ]] || fail "the device cgroup refused the helper its own node: $out"
[[ "$out" == *"not a hidraw node"* || "$out" == *"No such device"* || "$out" == *"No such device or address"* ]] \
    || fail "a node that is not hidraw was not refused as such: $out"
lamp_failed_cleanly "$cursor" || fail "the LampArray helper's unit did not run and exit 1 for a non-hidraw node"
echo "PASS lamp helper: its one node is bound in and allowed, and a non-hidraw node is refused"

# 5. Removal (ADR 0034): the battery gets back the limit it had before
# Better OS, and the choices and the boot-time restore's input are gone.
echo 80 > "$bat/charge_control_end_threshold"
echo '{"charge_limit":80}' > /var/lib/better-os/hardware.json
echo '{"charge_limit":100}' > /var/lib/better-os/hardware-before.json
/usr/libexec/better-manager-daemon --release-hardware
[[ "$(cat "$bat/charge_control_end_threshold")" == "100" ]] || fail "removal did not give the charge limit back"
[[ ! -e /var/lib/better-os/hardware.json && ! -e /var/lib/better-os/hardware-before.json ]] \
    || fail "removal left the charge limit's files"
systemctl start better-hardware-restore.service || true
[[ "$(systemctl show -p ConditionResult --value better-hardware-restore.service)" == "no" ]] \
    || fail "the boot-time restore still had something to write after removal"
echo "PASS removal: charge limit 80 -> 100 as before Better OS, nothing left for the boot-time restore"
# 6. The USB-C recovery's helpers (ADR 0046, ticket 101). A container shares
# the host's kernel, so nothing here may unload a module or unbind a real
# device: the module reload helper is never run with a module system call to
# make, and the rebind helper writes only into stand-ins mounted over
# /sys/bus/pci first. What is checked is the units' shape.
for cover in /sys/bus/pci/drivers /sys/bus/pci/devices; do
    [[ -d "$cover" ]] || { echo "FAIL: $cover is missing in this kernel" >&2; exit 1; }
    mount -t tmpfs -o size=1m,mode=0755 better-os-standin "$cover"
    if [[ "$(findmnt -n -o SOURCE --mountpoint "$cover")" != "better-os-standin" ]]; then
        echo "FAIL: $cover is not covered by the stand-in; nothing was written" >&2
        exit 1
    fi
done
xhci=/sys/bus/pci/devices/0000:63:00.4
mkdir -p "$xhci" /sys/bus/pci/drivers/xhci_hcd
echo 0x1022 > "$xhci/vendor"
echo 0x151e > "$xhci/device"
echo 0x0c0330 > "$xhci/class"
: > /sys/bus/pci/drivers/xhci_hcd/unbind
: > /sys/bus/pci/drivers/xhci_hcd/bind

# 6a. The reload helper's unit: systemd takes every property, /sys holds
# nothing but the kernel's /sys/module/compression (bound read-only when
# the kernel has it),
# the modules tree is readable, /proc/sys is read-only, and the only
# capability left is CAP_SYS_MODULE (bit 16). The helper itself is started
# only with an argument it refuses before any system call.
# A minimal container image ships no kernel modules, so the tree the unit
# has to leave readable is made here; the probe then shows the sandbox does
# not hide it.
mkdir -p /usr/lib/modules
props_for ucsi-reload
printf '%s\n' "${probe_props[@]}" | grep -qx 'ProtectKernelModules=yes' \
    && fail "the reload helper's list takes module loading away"
printf '%s\n' "${probe_props[@]}" | grep -qx 'CapabilityBoundingSet=CAP_SYS_MODULE' \
    || fail "the reload helper's list keeps more than CAP_SYS_MODULE"
probe='echo PROBE-STARTED'
probe="$probe; [ \"\$(find /sys -mindepth 1 ! -path /sys/module ! -path /sys/module/compression | wc -l)\" -eq 0 ] && echo SYS-EMPTY"
probe="$probe; [ -d /usr/lib/modules ] && echo MODULES-VISIBLE"
probe="$probe; grep -q ' /proc/sys ro,' /proc/self/mountinfo && echo PROC-SYS-RO"
probe="$probe; grep -q '^CapBnd:[[:space:]]*0000000000010000\$' /proc/self/status && echo CAP-MODULE-ONLY"
probe="$probe; echo /bin/false > /proc/sys/kernel/modprobe && echo WROTE-MODPROBE"
probe="$probe; touch /usr/lib/better-os-probe && echo WROTE-USR"
probe="$probe; true"
out="$(systemd-run --quiet --wait --pipe --collect "${probe_props[@]}" /bin/sh -c "$probe" 2>&1 || true)"
for wanted in PROBE-STARTED SYS-EMPTY MODULES-VISIBLE PROC-SYS-RO CAP-MODULE-ONLY; do
    [[ "$out" == *"$wanted"* ]] || fail "the reload helper's unit did not show $wanted: $out"
done
for marker in WROTE-MODPROBE WROTE-USR; do
    [[ "$out" != *"$marker"* ]] || fail "the reload helper's sandbox let through $marker: $out"
done
out="$(systemd-run --quiet --wait --pipe --collect "${probe_props[@]}" \
    /usr/libexec/better-manager-daemon --reload-ucsi refuse-me 2>&1 || true)"
[[ "$out" == *"takes no value"* ]] || fail "the reload helper did not start in its unit: $out"
echo "PASS reload helper: properties accepted; /sys holds only module/compression, modules readable, CAP_SYS_MODULE only"

# 6a'. The check before a reload has to run in the helper's unit: the daemon's
# own unit has ProtectKernelModules=yes, which hides /usr/lib/modules, and a
# check made there refuses every reload ("ucsi_acpi is not under
# /lib/modules/<release>", seen on the owner's machine with 0.2.17). A
# stand-in module file for the running kernel: the helper's unit finds it,
# and a unit with the daemon's property does not.
ucsi_dir="/usr/lib/modules/$(uname -r)/kernel/drivers/usb/typec/ucsi"
mkdir -p "$ucsi_dir"
: > "$ucsi_dir/ucsi_acpi.ko"
chmod 644 "$ucsi_dir/ucsi_acpi.ko"
out="$(systemd-run --quiet --wait --pipe --collect "${probe_props[@]}" \
    /usr/libexec/better-manager-daemon --check-ucsi 2>&1 || true)"
[[ "$out" == *"ready"* ]] \
    || fail "the helper's unit did not find the module file (expected: ready; found: $out)"
out="$(systemd-run --quiet --wait --pipe --collect -p ProtectKernelModules=yes \
    /usr/libexec/better-manager-daemon --check-ucsi 2>&1 || true)"
[[ "$out" == *"is not under"* ]] \
    || fail "a unit with ProtectKernelModules=yes was expected not to see the module file; found: $out"
grep -q '^ProtectKernelModules=yes' /usr/lib/systemd/system/better-manager-daemon.service \
    || fail "the daemon's unit no longer sets ProtectKernelModules=yes: this check and the helper's check unit need another look"
rm -f "$ucsi_dir/ucsi_acpi.ko"
echo "PASS driver check: found in the helper's unit, hidden from a unit like the daemon's"

# 6b. The rebind helper: its unit sees the driver's two files and the
# function's identity and nothing else, cannot change the identity, and the
# real helper rebinds the stand-in function by its address.
props_for xhci-rebind 0000:63:00.4
probe='echo PROBE-STARTED'
probe="$probe; [ \"\$(ls -A /sys | wc -l)\" -eq 1 ] && echo SYS-ONE-ENTRY"
probe="$probe; [ \"\$(ls -A /sys/bus/pci/drivers/xhci_hcd | wc -l)\" -eq 2 ] && echo DRIVER-TWO-FILES"
probe="$probe; echo 0x1234 > $xhci/device && echo WROTE-IDENTITY"
probe="$probe; grep -q ' /proc/sys ro,' /proc/self/mountinfo && echo PROC-SYS-RO"
probe="$probe; grep -q '^CapBnd:[[:space:]]*0000000000000000\$' /proc/self/status && echo NO-CAPS"
probe="$probe; true"
out="$(systemd-run --quiet --wait --pipe --collect "${probe_props[@]}" /bin/sh -c "$probe" 2>&1 || true)"
for wanted in PROBE-STARTED SYS-ONE-ENTRY DRIVER-TWO-FILES PROC-SYS-RO NO-CAPS; do
    [[ "$out" == *"$wanted"* ]] || fail "the rebind helper's unit did not show $wanted: $out"
done
[[ "$out" != *WROTE-IDENTITY* && "$(cat "$xhci/device")" == "0x151e" ]] \
    || fail "the rebind helper's unit could change the function's identity: $out"
out="$(systemd-run --quiet --wait --pipe --collect "${probe_props[@]}" \
    /usr/libexec/better-manager-daemon --rebind-xhci 0000:63:00.4 2>&1 || true)"
[[ "$out" == *rebound* ]] || fail "the rebind helper did not run in its unit: $out"
[[ "$(cat /sys/bus/pci/drivers/xhci_hcd/unbind)" == "0000:63:00.4" \
    && "$(cat /sys/bus/pci/drivers/xhci_hcd/bind)" == "0000:63:00.4" ]] \
    || fail "the rebind helper did not write unbind and bind"
echo 0x151f > "$xhci/device"
: > /sys/bus/pci/drivers/xhci_hcd/unbind
out="$(systemd-run --quiet --wait --pipe --collect "${probe_props[@]}" \
    /usr/libexec/better-manager-daemon --rebind-xhci 0000:63:00.4 2>&1 || true)"
[[ "$out" == *"not a controller of the affected kind"* && ! -s /sys/bus/pci/drivers/xhci_hcd/unbind ]] \
    || fail "the rebind helper touched a controller of another kind: $out"
echo "PASS rebind helper: two driver files and a read-only identity; rebinds only the affected kind"

# 6c. The after-sleep run's unit: systemd takes every property; it cannot
# write /etc, /var, or load modules, and can write /run/better-os.
mkdir -p /run/better-os
props_for after-sleep
probe='echo PROBE-STARTED'
probe="$probe; touch /etc/better-os-probe && echo WROTE-ETC"
probe="$probe; touch /var/lib/better-os-probe && echo WROTE-VAR"
probe="$probe; touch /run/better-os/probe && echo WROTE-RUN"
probe="$probe; grep -q '^CapBnd:[[:space:]]*00000000000000c0\$' /proc/self/status && echo CAPS-SETUID-SETGID"
probe="$probe; true"
out="$(systemd-run --quiet --wait --pipe --collect "${probe_props[@]}" /bin/sh -c "$probe" 2>&1 || true)"
for wanted in PROBE-STARTED WROTE-RUN CAPS-SETUID-SETGID; do
    [[ "$out" == *"$wanted"* ]] || fail "the after-sleep unit did not show $wanted: $out"
done
for marker in WROTE-ETC WROTE-VAR; do
    [[ "$out" != *"$marker"* ]] || fail "the after-sleep unit let through $marker: $out"
done
# With nothing chosen the hook queues nothing.
/usr/libexec/better-manager-daemon --after-sleep post suspend
! systemctl list-units --all --no-legend 'better-os-usb-after-sleep*' | grep -q . \
    || fail "the hook queued the after-sleep run with nothing chosen"
echo "PASS after-sleep: properties accepted; writes only /run/better-os; the hook does nothing while off"

# 6d. Removal forgets the choices.
echo '{"version":1,"ucsi_automatic":true}' > /var/lib/better-os/usb-recovery.json
/usr/libexec/better-manager-daemon --release-hardware
[[ ! -e /var/lib/better-os/usb-recovery.json ]] || fail "removal left the USB-C recovery's choices"
echo "PASS removal: the USB-C recovery's choices are gone"
systemctl --version | head -n 1
