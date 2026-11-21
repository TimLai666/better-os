#!/usr/bin/env bash
# Run as root inside a disposable VM. Records a person's home (every file with
# its checksum, caches left out) and what GNOME's dconf database holds, so two
# snapshots can be diffed: what a session wrote that GNOME reads.
#
#   home-snapshot.sh USER OUTDIR NAME
#   diff OUTDIR/home-A.tsv OUTDIR/home-B.tsv; diff OUTDIR/dconf-user-A.ini ...
set -euo pipefail
U="$1"; OUT="$2"; N="$3"; mkdir -p "$OUT"
H="$(getent passwd "$U" | cut -d: -f6)"
UID_="$(id -u "$U")"
(cd "$H" && find . -path ./.cache -prune -o -type f -print0 | sort -z | xargs -0 -r sha256sum) >"$OUT/home-$N.tsv"
(cd "$H" && find . -path ./.cache -prune -o -print | sort) >"$OUT/home-paths-$N.txt"
# GNOME's own database, read the way GNOME reads it (no DCONF_PROFILE).
runuser -u "$U" -- env -u DCONF_PROFILE XDG_RUNTIME_DIR="/run/user/$UID_" dconf dump / >"$OUT/dconf-user-$N.ini" 2>&1 || true
runuser -u "$U" -- env DCONF_PROFILE=/usr/share/dconf/profile/betteros XDG_RUNTIME_DIR="/run/user/$UID_" dconf dump / >"$OUT/dconf-betteros-$N.ini" 2>&1 || true
ls -la "$H/.config/dconf/" >"$OUT/dconf-files-$N.txt" 2>&1 || true
echo "snapshot $N: $(wc -l <"$OUT/home-$N.tsv") files"
