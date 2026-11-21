#!/usr/bin/env bash
# Run as root inside a disposable VM. Ticket 203's records of the real
# session package: which session units and Better OS units run, the user
# manager's environment, the environment of a process started from the
# compositor (a terminal) and of a D-Bus-activated service, and GNOME's and
# the session's dconf databases.
#
#   session-check.sh USER OUTDIR NAME
#
# NAME labels the moment (in-session, after-logout, in-gnome).
set -uo pipefail
U="$1"; OUT="$2/$3"; mkdir -p "$OUT"
UID_="$(id -u "$U")"
H="$(getent passwd "$U" | cut -d: -f6)"
export XDG_RUNTIME_DIR="/run/user/$UID_" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$UID_/bus"
as_user() { runuser -u "$U" -- env XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" DBUS_SESSION_BUS_ADDRESS="$DBUS_SESSION_BUS_ADDRESS" "$@"; }

{
    echo "== sessions"; loginctl list-sessions --no-legend
    for s in $(loginctl list-sessions --no-legend | awk '{print $1}'); do
        loginctl show-session "$s" -p Name -p Type -p Desktop -p State -p Leader -p Scope
    done
    echo "== the session's processes"
    ps -u "$U" -o pid,ppid,unit:40,args --forest
} >"$OUT/sessions.txt" 2>&1

{
    echo "== the session's units"
    as_user systemctl --user list-units --all --no-pager --plain 'better-os-session*' graphical-session.target \
        graphical-session-pre.target xdg-desktop-autostart.target tracker-miner-fs-3.service
    echo "== Better OS components' units"
    as_user systemctl --user list-units --all --no-pager --plain 'better-*' 'awake*'
    echo "== failed"
    as_user systemctl --user list-units --state=failed --no-pager --plain
    echo "== dependencies of better-os-session.target"
    as_user systemctl --user list-dependencies better-os-session.target --no-pager
} >"$OUT/units.txt" 2>&1

as_user systemctl --user show-environment >"$OUT/user-manager-environment.txt" 2>&1

# A process the compositor started (a terminal from Super+T, or anything
# else running under cosmic-comp), and a D-Bus-activated service.
{
    for name in cosmic-comp cosmic-greeter bash xdg-desktop-portal fcitx5 cosmic-settings-daemon better-launcher; do
        # -f with an anchored pattern: -x matches only the first 15 bytes.
        pid="$(pgrep -u "$U" -n -f "^(/[^ ]*/)?$name( |$)")" || { echo "== $name: not running"; continue; }
        echo "== $name (pid $pid, unit $(ps -o unit= -p "$pid"))"
        tr '\0' '\n' <"/proc/$pid/environ" | grep -E '^(XDG_CURRENT_DESKTOP|XDG_SESSION_DESKTOP|DESKTOP_SESSION|DCONF_PROFILE|XDG_DATA_DIRS|GTK_IM_MODULE|QT_IM_MODULE|XMODIFIERS|WAYLAND_DISPLAY|DISPLAY|BETTER_OS_SESSION_RESUME)=' | sort
    done
} >"$OUT/process-environments.txt" 2>&1

{
    echo "== org.betteros.Session1 and com.system76.CosmicSession owners"
    as_user busctl --user status org.betteros.Session1 2>&1 | grep -E '^(PID|Comm|Exe)='
    as_user busctl --user status com.system76.CosmicSession 2>&1 | grep -E '^(PID|Comm|Exe)='
    echo "== a Logout from busctl, which may not end the session"
    as_user busctl --user call org.betteros.Session1 /org/betteros/Session1 org.betteros.Session1 Logout 2>&1
} >"$OUT/session-bus.txt" 2>&1

{
    sha256sum "$H/.config/dconf/user" "$H/.config/dconf/betteros" 2>&1
    ls -la --time-style=full-iso "$H/.config/dconf/" 2>&1
} >"$OUT/dconf-files.txt"

journalctl -b --no-pager _UID="$UID_" 2>/dev/null | grep -F 'better-os-session:' >"$OUT/launcher-journal.txt"
journalctl -b --no-pager --user-unit='better-os-session*' _UID="$UID_" 2>/dev/null | tail -200 >"$OUT/session-units-journal.txt"
echo "recorded $3 in $OUT"
