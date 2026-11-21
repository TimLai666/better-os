#!/usr/bin/env bash
# Run as root inside a disposable VM. Takes out everything install-closure.sh
# and session-files.sh added, as the session's removal will
# (better-os-session.md, "Removing, and returning to the previous desktop"):
# purges every package the install added, the source, the key, the pin, the
# stand-in session files, and compares the package list with the one before.
#
#   remove-closure.sh OUTDIR     (OUTDIR is install-closure.sh's)
set -euo pipefail
OUT="$1"
export DEBIAN_FRONTEND=noninteractive
mapfile -t added <"$OUT/added-packages.txt"
apt-get -s purge "${added[@]}" >"$OUT/sim-purge.txt"
apt-get purge -y -q "${added[@]}" >"$OUT/purge.txt" 2>&1
rm -f /etc/apt/sources.list.d/better-os-system76.sources /etc/apt/preferences.d/better-os-system76.pref \
    /etc/apt/keyrings/better-os-system76.gpg
rm -f /var/lib/apt/lists/apt.pop-os.org_*
rm -f /usr/share/wayland-sessions/better-os.desktop /usr/lib/systemd/user/vm201-session.target \
    /usr/share/dconf/profile/betteros /usr/share/xdg-desktop-portal/betteros-portals.conf
rm -rf /usr/local/libexec/vm201
for owner in cosmic-greeter gdm3; do echo PURGE | debconf-communicate "$owner" >/dev/null 2>&1 || true; done
apt-get update -q >/dev/null 2>&1
dpkg-query -W -f='${Package}\t${Version}\t${Status}\n' | sort >"$OUT/dpkg-after-removal.tsv"
{
    echo "== package list before install vs after removal (diff; empty means identical)"
    diff "$OUT/dpkg-before.tsv" "$OUT/dpkg-after-removal.tsv" || true
    echo "== packages left in 'config-files' or other non-installed states"
    awk -F'\t' '$3 !~ / installed$/' "$OUT/dpkg-after-removal.tsv"
    echo "== apt-cache policy still naming System76"
    apt-cache policy | grep -i pop-os || echo none
    echo "== files left under /etc/apt naming System76"
    grep -rl -i 'pop-os\|system76' /etc/apt 2>/dev/null || echo none
    echo "== display-manager.service"; readlink -f /etc/systemd/system/display-manager.service
} >"$OUT/removal-summary.txt"
cat "$OUT/removal-summary.txt"
