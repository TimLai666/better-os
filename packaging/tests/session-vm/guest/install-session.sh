#!/usr/bin/env bash
# Run as root inside a disposable VM (never on a real machine). Ticket 203's
# install by hand: the pinned closure the way manager-daemon will add it
# (install-closure.sh: System76's source, its one key, the pin, GDM kept),
# then the built better-os-session package and the Better OS components from
# DEBDIR, then each component's user units enabled for every person, as
# Better Manager's enable step would for the person who installs them.
#
#   install-session.sh KEYRING DEBDIR OUTDIR
set -euo pipefail
KEYRING="$1"; DEBDIR="$2"; OUT="$3"; mkdir -p "$OUT"
HERE="$(cd "$(dirname "$0")" && pwd)"
export DEBIAN_FRONTEND=noninteractive

bash "$HERE/install-closure.sh" "$KEYRING" "$OUT/closure"

debs=()
for package in better-os-session better-manager better-manager-daemon better-shell-adapter \
    better-monitor better-launcher better-files better-git better-touchpad better-awake better-storage; do
    found=("$DEBDIR/${package}_"*.deb)
    [[ ${#found[@]} -eq 1 && -f "${found[0]}" ]] || { echo "no single $package package in $DEBDIR" >&2; exit 1; }
    debs+=("${found[0]}")
done
dpkg-query -W -f='${Package}\t${Version}\n' | sort >"$OUT/dpkg-before-session.tsv"
apt-get install -y -q "${debs[@]}" >"$OUT/install-session.txt" 2>&1
dpkg-query -W -f='${Package}\t${Version}\n' | sort >"$OUT/dpkg-after-session.tsv"
comm -13 "$OUT/dpkg-before-session.tsv" "$OUT/dpkg-after-session.tsv" >"$OUT/added-with-session.tsv"

# Better Manager enables a component's units for the person; --global does
# the same for every person, which is the stand-in for that one step here.
units=()
for package in better-monitor better-awake better-storage better-touchpad; do
    while IFS= read -r path; do
        [[ "$path" == /usr/lib/systemd/user/*.service ]] && units+=("${path##*/}")
    done < <(dpkg-query -L "$package")
done
systemctl --global enable "${units[@]}" >"$OUT/enable.txt" 2>&1
printf '%s\n' "${units[@]}" >"$OUT/component-units.txt"
echo "installed: ${#debs[@]} packages; enabled for everyone: ${units[*]}"
