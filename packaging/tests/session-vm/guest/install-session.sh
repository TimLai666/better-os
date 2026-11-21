#!/usr/bin/env bash
# Run as root inside a disposable VM (never on a real machine). Ticket 203's
# install by hand: the pinned closure the way manager-daemon will add it
# (install-closure.sh: System76's source, its one key, the pin, GDM kept),
# then the built better-os-session package and the Better OS components from
# DEBDIR, then each component's user units enabled for every person, as
# Better Manager's enable step would for the person who installs them.
# Last, check-shortcut-upstream.sh compares the installed COSMIC shortcut
# files with UPSTREAM_TSV (packaging/session/cosmic/upstream.tsv, copied into
# the guest), and once more against an altered copy, which must fail: the
# install fails when the closure moved past the session's shortcut copy.
#
#   install-session.sh KEYRING DEBDIR OUTDIR UPSTREAM_TSV
set -euo pipefail
KEYRING="$1"; DEBDIR="$2"; OUT="$3"; UPSTREAM="$4"; mkdir -p "$OUT"
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

# The session's shortcut directory replaces COSMIC's; the closure just
# installed must still ship the files it was made from.
if ! bash "$HERE/check-shortcut-upstream.sh" "$UPSTREAM" / >"$OUT/shortcut-upstream.txt" 2>&1; then
    cat "$OUT/shortcut-upstream.txt" >&2
    echo "the installed COSMIC shortcuts moved past the session's copy" >&2
    exit 1
fi
# The check itself: a copy of the files with one changed must fail it.
altered="$(mktemp -d)"
while IFS=$'\t' read -r session installed _; do
    [[ -z "$session" || "$session" == \#* ]] && continue
    mkdir -p "$altered$(dirname "$installed")"
    cp "$installed" "$altered$installed"
done <"$UPSTREAM"
first="$(awk -F'\t' '$1 !~ /^#/ && NF { print $2; exit }' "$UPSTREAM")"
printf '\n// altered\n' >>"$altered$first"
if bash "$HERE/check-shortcut-upstream.sh" "$UPSTREAM" "$altered" >"$OUT/shortcut-upstream-altered.txt" 2>&1; then
    echo "check-shortcut-upstream.sh passed a changed $first" >&2
    exit 1
fi
rm -rf "$altered"
echo "shortcut files: as recorded; an altered copy is refused"
