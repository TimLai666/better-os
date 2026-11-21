#!/usr/bin/env bash
# Run as root inside a disposable VM while USER is in the session under test.
# Exercises the session for MINUTES minutes the way a person's day would touch
# shared files: applications started and closed (GTK 3 and 4, Qt, a browser
# where one is installed, the file chooser and screenshot portals), files
# opened and saved, the COSMIC settings window opened on its appearance,
# display, keyboard, and input pages. Run home-snapshot.sh before and after
# and diff the two to see what the session wrote that GNOME reads.
#
#   hour-of-use.sh USER MINUTES
set -uo pipefail
U="$1"; MIN="${2:-60}"
HERE="$(cd "$(dirname "$0")" && pwd)"
run() { bash "$HERE/in-session.sh" "$U" "$@" >/dev/null 2>&1; }
stop() { pkill -u "$U" -f "$1" 2>/dev/null; }
H="$(getent passwd "$U" | cut -d: -f6)"
end=$(( $(date +%s) + MIN * 60 ))
i=0
while (( $(date +%s) < end )); do
    i=$((i + 1))
    run "use-edit-$i" gnome-text-editor "$H/Documents/note-$((i % 3)).txt"; sleep 20; stop gnome-text-editor
    run "use-term-$i" gnome-terminal -- sh -c 'ls ~; sleep 10'; sleep 15
    run "use-pick-$i" zenity --file-selection --title=pick; sleep 10; stop "zenity --file-selection"
    for page in appearance displays keyboard input-devices sound power; do
        run "use-settings-$i-$page" cosmic-settings "$page"; sleep 8; stop cosmic-settings
    done
    command -v firefox >/dev/null && { run "use-ff-$i" firefox about:blank; sleep 25; stop /snap/firefox; }
    command -v brave-browser >/dev/null && { run "use-br-$i" brave-browser --no-first-run about:blank; sleep 25; stop brave; }
    run "use-qt-$i" fcitx5-config-qt; sleep 10; stop fcitx5-config-qt
    echo "round $i done $(date -Is)"
done
