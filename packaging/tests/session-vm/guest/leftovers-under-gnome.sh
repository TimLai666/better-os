#!/usr/bin/env bash
# Run as root inside a disposable VM (never on a real machine). Ticket 203's
# second review, P2: a Better OS session whose launcher died with its guard
# already gone leaves its record and environment for the leftovers unit,
# which then runs inside the next GNOME login, after GNOME's login has begun
# importing its own values. The unit must put back only what is still the
# session's and must not stop graphical-session.target under GNOME.
#
#   leftovers-under-gnome.sh kill USER OUT    in a running Better OS session:
#       linger on, the user manager's environment saved, the guard stopped,
#       the launcher killed with SIGKILL, what is left saved
#   leftovers-under-gnome.sh after USER OUT   once logged in to GNOME: the
#       user manager's and GNOME Shell's environment, the record, the units,
#       and the journal of the login's jobs saved
set -euo pipefail
MODE="$1"; USER_NAME="$2"; OUT="$3"; mkdir -p "$OUT"
uid="$(id -u "$USER_NAME")"
as_user() {
    runuser -u "$USER_NAME" -- env XDG_RUNTIME_DIR="/run/user/$uid" \
        DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$uid/bus" "$@"
}
units=(better-os-session.target better-os-session-autostart.target
    better-os-session-guard.service better-os-session-leftovers.service
    graphical-session.target graphical-session-pre.target gnome-session.target
    xdg-desktop-autostart.target tracker-miner-fs-3.service)

case "$MODE" in
kill)
    loginctl enable-linger "$USER_NAME"
    as_user systemctl --user show-environment >"$OUT/1-env-in-session.txt"
    # -f anchored: the guard runs the same program (`start --guard`).
    launcher="$(pgrep -u "$uid" -f '^/usr/libexec/better-os/session/start$' || true)"
    [[ -n "$launcher" ]] || { echo "no launcher runs for $USER_NAME" >&2; exit 1; }
    echo "$launcher" >"$OUT/launcher-pid.txt"
    as_user systemctl --user stop better-os-session-guard.service
    as_user systemctl --user is-active better-os-session-guard.service \
        >"$OUT/2-guard-after-stop.txt" 2>&1 || true
    kill -9 "$launcher"
    sleep 5
    as_user systemctl --user show-environment >"$OUT/3-env-after-kill.txt"
    as_user systemctl --user list-units --all --no-pager "${units[@]}" \
        >"$OUT/3-units-after-kill.txt" 2>&1 || true
    ls -l "/run/user/$uid/better-os/session/" >"$OUT/3-record-after-kill.txt" 2>&1 || true
    loginctl list-sessions --no-pager >"$OUT/3-sessions-after-kill.txt"
    date --iso-8601=seconds >"$OUT/killed-at.txt"
    echo "killed launcher $launcher; log in to GNOME, then: $0 after $USER_NAME OUT"
    ;;
after)
    as_user systemctl --user show-environment >"$OUT/4-env-in-gnome.txt"
    shell="$(pgrep -u "$uid" -x gnome-shell | head -1)"
    tr '\0' '\n' <"/proc/$shell/environ" | sort >"$OUT/4-gnome-shell-environ.txt"
    as_user systemctl --user list-units --all --no-pager "${units[@]}" \
        >"$OUT/4-units-in-gnome.txt" 2>&1 || true
    ls -l "/run/user/$uid/better-os/session/" >"$OUT/4-record-in-gnome.txt" 2>&1 || true
    since="$(cat "$OUT/killed-at.txt")"
    journalctl --no-pager -o short-precise _UID="$uid" --since "$since" \
        >"$OUT/4-user-journal.txt" 2>&1 || true
    journalctl --no-pager -o short-precise --user-unit better-os-session-leftovers.service \
        --since "$since" >"$OUT/4-leftovers-journal.txt" 2>&1 || true
    # The jobs of the GNOME login: what started and stopped around
    # gnome-session.target and graphical-session.target.
    grep -E 'gnome-session|graphical-session|better-os-session|leftovers|cycle' \
        "$OUT/4-user-journal.txt" >"$OUT/4-login-jobs.txt" || true
    for name in DCONF_PROFILE XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP DESKTOP_SESSION \
        XDG_DATA_DIRS WAYLAND_DISPLAY DISPLAY QT_IM_MODULE XMODIFIERS; do
        printf '%s\tmanager=%s\tshell=%s\n' "$name" \
            "$(grep -m1 "^$name=" "$OUT/4-env-in-gnome.txt" | cut -d= -f2- || true)" \
            "$(grep -m1 "^$name=" "$OUT/4-gnome-shell-environ.txt" | cut -d= -f2- || true)"
    done >"$OUT/summary.tsv"
    cat "$OUT/summary.tsv"
    ;;
*)
    echo "usage: $0 kill|after USER OUT" >&2; exit 2 ;;
esac
