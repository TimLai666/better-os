#!/usr/bin/env bash
# Disposable virtual machines for the Better OS session checks (tickets 201,
# 203 to 205). One VM at a time, headless, under a memory-capped user scope,
# ended by PID. Nothing here touches the host's packages or settings.
#
#   vm.sh ubuntu-create        Ubuntu 24.04 cloud image + cloud-init; installs
#                              ubuntu-desktop-minimal (GNOME, GDM) inside and
#                              snapshots the disk as "base"
#   vm.sh ubuntu-boot          boot the Ubuntu disk (graphical, GDM)
#   vm.sh zorin-boot           boot the Zorin OS 18 Core ISO as a live system
#                              (no disk; the live session lives in RAM)
#   vm.sh stop                 power off through QMP, then end the PID
#   vm.sh ssh [COMMAND...]     run a command as root in the guest
#   vm.sh scp-from GUEST HOST  copy a file out of the guest
#   vm.sh scp-to HOST GUEST    copy a file into the guest
#   vm.sh shot OUT.png         screendump of the guest's display
#   vm.sh key COMBO...         QMP send-key, one combo per argument (ctrl-alt-t)
#   vm.sh type TEXT            type ASCII text through QMP
#   vm.sh click X Y [right]    click at a pixel of the 1920x1080 display
#   vm.sh snapshot NAME        internal qcow2 snapshot (VM must be stopped)
#   vm.sh revert NAME          go back to a snapshot (VM must be stopped)
#   vm.sh delete               delete the disk image and say how much it freed
#
# Environment: VMDIR (default ~/.cache/better-os/orch/vm201), holding
#   dl-ubuntu/noble-server-cloudimg-amd64.img   (checksum and signature
#                                                verified by the caller)
#   dl-zorin/Zorin-OS-18.1-Core-64-bit.iso       (checksum verified)
# Every image is a sparse qcow2 capped at 24 GiB, written with discard=unmap.
# The script refuses to create or boot when / has less than 12 GiB free.
#
# The run ticket 201 made (repeat it for 203 to 205, replacing the stand-in
# session files with the built package). The guest scripts are in guest/;
# copy them to /root/g in the VM with scp-to.
#   1. Downloads: the cloud image with SHA256SUMS checked against its gpgv
#      signature (/usr/share/keyrings/ubuntu-cloudimage-keyring.gpg); the
#      Zorin ISO from a mirror zorin.com/os/download/18/core/ lists, checked
#      against help.zorin.com's published SHA256. System76's key from
#      keyserver.ubuntu.com, exported to a keyring holding only
#      63C46DF0140D738961429F4E204DD8AEC33A7AFF and checked against
#      apt.pop-os.org/release/dists/noble/InRelease.
#   2. ubuntu-create (or zorin-boot, then in the live terminal: install
#      openssh-server and root's authorized_keys from id_ed25519.pub, and in
#      /etc/gdm3/custom.conf drop casper's WaylandEnable=false and autologin,
#      then restart gdm: an installed Zorin OS 18 has Wayland on).
#   3. Log in to GNOME once (key ret, type the password from test-password,
#      key ret); guest/home-snapshot.sh USER OUT A.
#   4. guest/install-closure.sh KEYRING OUT/closure; guest/session-files.sh
#      USER. GDM lists the session only in a greeter started after the file
#      exists (systemctl restart gdm while nobody is logged in).
#   5. In GDM: click the gear (1887,1047), choose Better OS, log in;
#      guest/inventory.sh USER OUT/inv; the input, lock, screen-cast, Orca and
#      polkit checks with guest/in-session.sh; guest/hour-of-use.sh USER 60;
#      home-snapshot B; log out; log in to GNOME; home-snapshot C; diff.
#   6. guest/remove-closure.sh OUT/closure and its diff; stop; delete.
set -euo pipefail

VMDIR="${VMDIR:-$HOME/.cache/better-os/orch/vm201}"
DISK="$VMDIR/disk.qcow2"
KEY="$VMDIR/id_ed25519"
SSHPORT="${SSHPORT:-2201}"
CAP=24G
MIN_FREE_GIB=12
HERE="$(cd "$(dirname "$0")" && pwd)"
# The guest has only C.UTF-8; ssh forwards the host's locale variables.
export LC_ALL=C.UTF-8 LANG=C.UTF-8
# Programs the checks use that are not part of what is being checked: a GTK
# dialog that prints what was typed, Wayland introspection, an X11 client,
# Orca, screen-cast and PipeWire tools, and unzip for the Electron fixture.
GUEST_TOOLS="zenity wayland-utils x11-utils xterm orca pipewire-bin gstreamer1.0-pipewire unzip dconf-cli gnome-terminal gnome-text-editor"

die() { printf 'vm.sh: %s\n' "$1" >&2; exit 1; }

free_gib() { df --output=avail -B1G / | tail -1 | tr -d ' '; }
need_space() {
    local f; f="$(free_gib)"
    ((f >= MIN_FREE_GIB)) || die "only ${f} GiB free on /; refusing (minimum ${MIN_FREE_GIB})"
    printf 'free on /: %s GiB\n' "$f"
}

running_pid() {
    [[ -f "$VMDIR/qemu.pid" ]] || return 1
    local pid; pid="$(cat "$VMDIR/qemu.pid")"
    kill -0 "$pid" 2>/dev/null && printf '%s' "$pid"
}

qmp() {
    # Each argument is one JSON command; the replies are printed.
    local cmds='{"execute":"qmp_capabilities"}'
    for c in "$@"; do cmds+=$'\n'"$c"; done
    printf '%s\n' "$cmds" | socat -t 3 - UNIX-CONNECT:"$VMDIR/qmp.sock" | tail -n +2
}

launch() {
    # $@: extra qemu arguments. Runs detached in a capped user scope.
    running_pid >/dev/null && die "a VM is already running (PID $(running_pid))"
    rm -f "$VMDIR/qmp.sock" "$VMDIR/serial.log"
    systemd-run --user --scope --quiet -p MemoryMax=7G -p MemorySwapMax=0 \
        --unit="better-os-vm201-$$" \
        qemu-system-x86_64 -enable-kvm -cpu host -smp 4 -m 6G -machine q35 \
        -device virtio-vga,xres=1920,yres=1080 -display none \
        -device qemu-xhci -device usb-tablet -device usb-kbd \
        -device intel-hda -device hda-duplex,audiodev=snd0 -audiodev none,id=snd0 \
        -nic "user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:${SSHPORT}-:22" \
        -serial file:"$VMDIR/serial.log" \
        -qmp unix:"$VMDIR/qmp.sock",server,nowait \
        -pidfile "$VMDIR/qemu.pid" -daemonize "$@"
    printf 'qemu PID %s\n' "$(cat "$VMDIR/qemu.pid")"
}

ssh_opts=(-i "$KEY" -p "$SSHPORT" -o StrictHostKeyChecking=no
    -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=5)

wait_ssh() {
    local i
    for i in $(seq 1 "${1:-180}"); do
        ssh "${ssh_opts[@]}" root@127.0.0.1 true 2>/dev/null && return 0
        sleep 5
    done
    die "no ssh after $((${1:-180} * 5)) s; see $VMDIR/serial.log"
}

keypair() {
    [[ -f "$KEY" ]] || ssh-keygen -q -t ed25519 -N '' -C vm201 -f "$KEY"
    # The test account's password exists only inside the VM; it is generated
    # here and kept in the VM directory so a GDM login can be typed.
    [[ -f "$VMDIR/test-password" ]] || { head -c 9 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' >"$VMDIR/test-password"; chmod 600 "$VMDIR/test-password"; }
}

ubuntu_create() {
    local img="$VMDIR/dl-ubuntu/noble-server-cloudimg-amd64.img"
    [[ -f "$img" ]] || die "missing $img"
    [[ -f "$DISK" ]] && die "$DISK exists; delete it first (one installed image at a time)"
    need_space
    keypair
    qemu-img create -q -f qcow2 -o cluster_size=65536 -b "$img" -F qcow2 "$DISK" "$CAP"
    local seed="$VMDIR/seed"; rm -rf "$seed"; mkdir -p "$seed"
    local pub; pub="$(cat "$KEY.pub")"
    local pw; pw="$(cat "$VMDIR/test-password")"
    cat >"$seed/user-data" <<EOF
#cloud-config
hostname: vm201
users:
  - name: root
    ssh_authorized_keys: ["$pub"]
  - name: tester
    gecos: Tester
    shell: /bin/bash
    groups: [sudo, adm]
    lock_passwd: false
    ssh_authorized_keys: ["$pub"]
chpasswd:
  expire: false
  users:
    - {name: tester, password: "$pw", type: text}
disable_root: false
ssh_pwauth: false
package_update: false
EOF
    printf 'instance-id: vm201\nlocal-hostname: vm201\n' >"$seed/meta-data"
    genisoimage -quiet -output "$VMDIR/seed.iso" -volid cidata -joliet -rock "$seed/user-data" "$seed/meta-data"
    launch -drive file="$DISK",if=virtio,discard=unmap,detect-zeroes=unmap \
        -drive file="$VMDIR/seed.iso",media=cdrom,readonly=on
    wait_ssh
    # GNOME and GDM as Ubuntu ships them for a desktop: ubuntu-desktop-minimal
    # is the "minimal installation" choice of the desktop installer. Installing
    # it hands the network to NetworkManager, which leaves the interface
    # unmanaged until netplan is applied again and drops this ssh connection,
    # so the install runs detached in the guest and is polled.
    ssh "${ssh_opts[@]}" root@127.0.0.1 'cloud-init status --wait >/dev/null || true
        systemd-run --unit=vm201-desktop --property=Type=oneshot --no-block sh -c "
            export DEBIAN_FRONTEND=noninteractive
            apt-get update -q && apt-get dist-upgrade -y -q &&
            apt-get install -y -q ubuntu-desktop-minimal '"$GUEST_TOOLS"' &&
            systemctl set-default graphical.target &&
            touch /etc/cloud/cloud-init.disabled && apt-get clean
            echo vm201-desktop-exit=\$?
            netplan apply" >/var/log/vm201-desktop-install.log 2>&1'
    sleep 60
    for _ in $(seq 1 240); do
        st="$(ssh "${ssh_opts[@]}" root@127.0.0.1 'systemctl show -p ActiveState --value vm201-desktop' 2>/dev/null || true)"
        [[ "$st" == inactive || "$st" == failed ]] && break
        sleep 15
    done
    ssh "${ssh_opts[@]}" root@127.0.0.1 'journalctl -u vm201-desktop --no-pager | grep vm201-desktop-exit; fstrim -av' || true
    stop
    qemu-img snapshot -c base "$DISK"
    qemu-img info "$DISK" | grep -E 'virtual size|disk size'
}

ubuntu_boot() {
    [[ -f "$DISK" ]] || die "no disk; run ubuntu-create"
    need_space
    launch -drive file="$DISK",if=virtio,discard=unmap,detect-zeroes=unmap
    wait_ssh
}

zorin_boot() {
    local iso="$VMDIR/dl-zorin/Zorin-OS-18.1-Core-64-bit.iso"
    [[ -f "$iso" ]] || die "missing $iso"
    need_space
    keypair
    # Boot the live system's own kernel and initrd straight from the ISO, with
    # the serial console added, as ticket 143 booted Pop!_OS.
    local boot="$VMDIR/zorin-boot"; mkdir -p "$boot"
    [[ -f "$boot/vmlinuz" ]] || xorriso -osirrox on -indev "$iso" \
        -extract /casper/vmlinuz "$boot/vmlinuz" -extract /casper/initrd.zstd "$boot/initrd" 2>/dev/null
    launch -kernel "$boot/vmlinuz" -initrd "$boot/initrd" \
        -append "boot=casper quiet splash console=tty0 console=ttyS0,115200" \
        -drive file="$iso",media=cdrom,readonly=on,if=none,id=cd \
        -device ahci,id=ahci -device ide-cd,drive=cd,bus=ahci.0
}

stop() {
    local pid
    pid="$(running_pid)" || { echo "no VM running"; return 0; }
    ssh "${ssh_opts[@]}" root@127.0.0.1 'systemctl poweroff' 2>/dev/null || qmp '{"execute":"system_powerdown"}' >/dev/null || true
    for _ in $(seq 1 60); do kill -0 "$pid" 2>/dev/null || break; sleep 2; done
    kill -0 "$pid" 2>/dev/null && { kill "$pid"; sleep 2; }
    kill -0 "$pid" 2>/dev/null && kill -9 "$pid"
    rm -f "$VMDIR/qemu.pid"
    echo "VM $pid ended"
}

shot() {
    local out; out="$(realpath -m "$1")"
    qmp "{\"execute\":\"screendump\",\"arguments\":{\"filename\":\"$out\",\"format\":\"png\"}}" >/dev/null
    sleep 1; ls -la "$out"
}

key() {
    local combo k json
    for combo in "$@"; do
        json='['
        IFS=- read -ra ks <<<"$combo"
        for k in "${ks[@]}"; do json+="{\"type\":\"qcode\",\"data\":\"$k\"},"; done
        json="${json%,}]"
        qmp "{\"execute\":\"send-key\",\"arguments\":{\"keys\":$json}}" >/dev/null
        sleep 0.08
    done
}

click() {
    # $1 $2: pixel position on the 1920x1080 display; $3: left (default) or right
    local x=$(($1 * 32767 / 1919)) y=$(($2 * 32767 / 1079)) b="${3:-left}"
    qmp "{\"execute\":\"input-send-event\",\"arguments\":{\"events\":[{\"type\":\"abs\",\"data\":{\"axis\":\"x\",\"value\":$x}},{\"type\":\"abs\",\"data\":{\"axis\":\"y\",\"value\":$y}}]}}" \
        "{\"execute\":\"input-send-event\",\"arguments\":{\"events\":[{\"type\":\"btn\",\"data\":{\"down\":true,\"button\":\"$b\"}}]}}" \
        "{\"execute\":\"input-send-event\",\"arguments\":{\"events\":[{\"type\":\"btn\",\"data\":{\"down\":false,\"button\":\"$b\"}}]}}" >/dev/null
}

type_text() {
    local s="$1" i ch
    declare -A plain=([' ']=spc [-]=minus [=]=equal [.]=dot [,]=comma [/]=slash [';']=semicolon
        ["'"]=apostrophe ['[']=bracket_left [']']=bracket_right ['\']=backslash ['`']=grave_accent)
    declare -A shifted=([_]=minus [+]=equal ['>']=dot ['<']=comma ['?']=slash [':']=semicolon
        ['"']=apostrophe ['{']=bracket_left ['}']=bracket_right ['|']=backslash ['~']=grave_accent
        ['!']=1 ['@']=2 ['#']=3 ['$']=4 ['%']=5 ['^']=6 ['&']=7 ['*']=8 ['(']=9 [')']=0)
    for ((i = 0; i < ${#s}; i++)); do
        ch="${s:i:1}"
        if [[ "$ch" =~ [a-z0-9] ]]; then key "$ch"
        elif [[ "$ch" =~ [A-Z] ]]; then key "shift-${ch,,}"
        elif [[ -n "${plain[$ch]:-}" ]]; then key "${plain[$ch]}"
        elif [[ -n "${shifted[$ch]:-}" ]]; then key "shift-${shifted[$ch]}"
        else die "cannot type '$ch'"; fi
    done
}

delete() {
    running_pid >/dev/null && die "stop the VM first"
    local before after
    before="$(df --output=avail -B1M / | tail -1)"
    rm -f "$DISK" "$VMDIR/seed.iso"
    after="$(df --output=avail -B1M / | tail -1)"
    printf 'deleted %s; freed %s MiB\n' "$DISK" "$((after - before))"
}

cmd="${1:-}"; shift || true
case "$cmd" in
    ubuntu-create) ubuntu_create ;;
    ubuntu-boot) ubuntu_boot ;;
    zorin-boot) zorin_boot ;;
    stop) stop ;;
    ssh) ssh "${ssh_opts[@]}" root@127.0.0.1 "$@" ;;
    scp-from) scp -q "${ssh_opts[@]/-p/-P}" "root@127.0.0.1:$1" "$2" ;;
    scp-to) scp -q "${ssh_opts[@]/-p/-P}" "$1" "root@127.0.0.1:$2" ;;
    wait-ssh) wait_ssh "${1:-180}" ;;
    shot) shot "$1" ;;
    key) key "$@" ;;
    type) type_text "$1" ;;
    click) click "$@" ;;
    snapshot) running_pid >/dev/null && die "stop the VM first"; qemu-img snapshot -c "$1" "$DISK" ;;
    revert) running_pid >/dev/null && die "stop the VM first"; qemu-img snapshot -a "$1" "$DISK" ;;
    delete) delete ;;
    qmp) qmp "$@" ;;
    *) sed -n '2,/^set -euo/p' "$0" | sed '$d'; exit 2 ;;
esac
