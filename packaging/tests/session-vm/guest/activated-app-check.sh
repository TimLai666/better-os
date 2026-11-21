#!/usr/bin/env bash
# Run as root inside a disposable VM while USER is logged in. Starts GNOME
# Text Editor the way GNOME starts a DBusActivatable application, through
# D-Bus activation (its service file has no SystemdService=, so dbus-daemon
# starts it with the activation environment), records its DCONF_PROFILE and
# XDG_CURRENT_DESKTOP, lets it write its window state, and records which
# dconf database changed (ticket 203's review, P1 and P2).
#
#   activated-app-check.sh USER OUTDIR NAME
set -uo pipefail
U="$1"; OUT="$2/$3"; mkdir -p "$OUT"
UID_="$(id -u "$U")"
H="$(getent passwd "$U" | cut -d: -f6)"
as_user() { runuser -u "$U" -- env XDG_RUNTIME_DIR="/run/user/$UID_" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$UID_/bus" "$@"; }
{
    echo "== the service file"; grep -H . /usr/share/dbus-1/services/org.gnome.TextEditor.service
    echo "== which bus"; as_user systemctl --user show -p ExecStart dbus.service 2>&1 | head -2
    echo "== dconf databases before"; ls -la --time-style=full-iso "$H/.config/dconf/"; sha256sum "$H"/.config/dconf/*
} >"$OUT/before.txt" 2>&1
pkill -u "$U" -x gnome-text-edit 2>/dev/null; sleep 1
as_user busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus \
    StartServiceByName su org.gnome.TextEditor 0 >"$OUT/start.txt" 2>&1
sleep 4
pid="$(pgrep -u "$U" -n -f 'gnome-text-editor')"
{
    echo "== gnome-text-editor pid ${pid:-none}, unit $( [[ -n "$pid" ]] && ps -o unit= -p "$pid")"
    [[ -n "$pid" ]] && tr '\0' '\n' <"/proc/$pid/environ" | grep -E '^(DCONF_PROFILE|XDG_CURRENT_DESKTOP|XDG_DATA_DIRS|XDG_SESSION_DESKTOP)=' | sort
    echo "(no DCONF_PROFILE line above means it is unset)"
} >"$OUT/app-environment.txt" 2>&1
# Ask it to open a window and then quit, so it saves its state through GSettings.
as_user gdbus call --session --dest org.gnome.TextEditor --object-path /org/gnome/TextEditor \
    --method org.freedesktop.Application.Activate '{}' >>"$OUT/start.txt" 2>&1
sleep 3
as_user gdbus call --session --dest org.gnome.TextEditor --object-path /org/gnome/TextEditor \
    --method org.freedesktop.Application.ActivateAction quit '[]' '{}' >>"$OUT/start.txt" 2>&1
sleep 3
{
    echo "== dconf databases after"; ls -la --time-style=full-iso "$H/.config/dconf/"; sha256sum "$H"/.config/dconf/*
} >"$OUT/after.txt" 2>&1
cat "$OUT/app-environment.txt"
diff <(sha256sum "$H"/.config/dconf/* 2>/dev/null) <(grep -E '^[0-9a-f]{64} ' "$OUT/before.txt") && echo "no dconf database changed" || echo "dconf databases changed (see before/after)"
