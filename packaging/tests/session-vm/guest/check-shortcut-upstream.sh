#!/usr/bin/env bash
# Run inside a disposable VM after the closure is installed. Fails when an
# installed COSMIC file that the session's shortcut directory replaces is not
# the one recorded in packaging/session/cosmic/upstream.tsv: the session's
# copy then hides System76's change (a new binding, a renamed action), and
# must be brought up to date before the session is shipped with that closure.
#
#   check-shortcut-upstream.sh UPSTREAM_TSV [ROOT]
#
# ROOT (default /) is where the installed files are looked for.
set -euo pipefail
TSV="$1"; ROOT="${2:-/}"
status=0
while IFS=$'\t' read -r session installed package version source license sha256; do
    [[ -z "$session" || "$session" == \#* ]] && continue
    file="${ROOT%/}$installed"
    if [[ ! -f "$file" ]]; then
        echo "MISSING $installed ($package): the closure no longer installs it; the session's $session replaces nothing and the record is stale" >&2
        status=1
        continue
    fi
    actual="$(sha256sum "$file" | awk '{print $1}')"
    if [[ "$actual" == "$sha256" ]]; then
        echo "same    $installed ($package $version)"
    else
        installed_version="$(dpkg-query -W -f='${Version}' "$package" 2>/dev/null || echo unknown)"
        {
            echo "CHANGED $installed: $package $installed_version ships sha256 $actual, the session's $session was made from $version ($sha256)."
            echo "        Compare the new file with $source's revision, carry every new binding or action into"
            echo "        packaging/session/cosmic/com.system76.CosmicSettings.Shortcuts/v1/$session (with the session's own changes kept),"
            echo "        and record the new version and hash in packaging/session/cosmic/upstream.tsv."
        } >&2
        status=1
    fi
done <"$TSV"
exit "$status"
