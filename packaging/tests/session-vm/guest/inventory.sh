#!/usr/bin/env bash
# Run as root inside a disposable VM while USER is logged in to the session
# under test. Records what runs and what the session's binaries need, for the
# duty inventory of better-os-session.md.
#
#   inventory.sh USER OUTDIR
set -uo pipefail
U="$1"; OUT="$2"; mkdir -p "$OUT"
UID_="$(id -u "$U")"
export XDG_RUNTIME_DIR="/run/user/$UID_" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$UID_/bus"
as_user() { runuser -u "$U" -- env XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" DBUS_SESSION_BUS_ADDRESS="$DBUS_SESSION_BUS_ADDRESS" "$@"; }

{
    echo "== loginctl"; loginctl list-sessions --no-legend
    for s in $(loginctl list-sessions --no-legend | awk '{print $1}'); do
        loginctl show-session "$s" -p Name -p Type -p Class -p Desktop -p Service -p State -p Remote -p Seat -p Leader
        echo
    done
    echo "== display-manager.service"; readlink -f /etc/systemd/system/display-manager.service
    systemctl is-active display-manager.service
} >"$OUT/sessions.txt" 2>&1

ps -u "$U" -o pid,ppid,etimes,rss,args --forest >"$OUT/processes.txt"
ps -eo pid,user,args | grep -E 'Xwayland|cosmic|fcitx|portal|keyring|polkit|tracker|orca|at-spi|pipewire|wireplumber|gsd-|gnome-shell|notif' | grep -v grep >"$OUT/processes-of-interest.txt"
as_user systemctl --user list-units --all --no-pager --plain >"$OUT/user-units.txt" 2>&1
as_user systemctl --user list-units --state=failed --no-pager --plain >"$OUT/user-units-failed.txt" 2>&1
as_user systemctl --user list-dependencies graphical-session.target --no-pager >"$OUT/graphical-session-deps.txt" 2>&1
as_user systemctl --user list-dependencies xdg-desktop-autostart.target --no-pager >"$OUT/autostart-target-deps.txt" 2>&1
as_user systemctl --user show-environment >"$OUT/user-manager-environment.txt" 2>&1
as_user busctl --user list --no-pager >"$OUT/session-bus-names.txt" 2>&1
busctl list --no-pager >"$OUT/system-bus-names.txt" 2>&1

# Where each well-known duty's bus name is answered from.
{
    for n in org.freedesktop.Notifications org.freedesktop.ScreenSaver org.freedesktop.secrets \
        org.freedesktop.portal.Desktop org.freedesktop.impl.portal.desktop.cosmic \
        org.freedesktop.impl.portal.desktop.gtk org.freedesktop.impl.portal.desktop.gnome \
        org.a11y.Bus org.kde.StatusNotifierWatcher org.fcitx.Fcitx5 org.freedesktop.FileManager1 \
        org.gnome.keyring org.freedesktop.Tracker3.Miner.Files com.system76.CosmicSettingsDaemon \
        org.gnome.SessionManager org.freedesktop.portal.IBus org.freedesktop.IBus; do
        pid="$(as_user busctl --user status "$n" 2>/dev/null | awk -F= '/^PID=/{print $2}')"
        printf '%-48s %s\n' "$n" "${pid:+$pid $(ps -o args= -p "$pid" | cut -c1-120)}${pid:-<no owner>}"
    done
} >"$OUT/duty-bus-names.txt" 2>&1

# Portals: which backend serves each interface.
{
    echo "== XDG_CURRENT_DESKTOP of xdg-desktop-portal"
    p="$(pgrep -u "$U" -x xdg-desktop-por | head -1)"
    [[ -n "$p" ]] && tr '\0' '\n' </proc/"$p"/environ | grep -E '^XDG_CURRENT_DESKTOP='
    echo "== portal configuration files"; ls -l /usr/share/xdg-desktop-portal/*.conf /etc/xdg/xdg-desktop-portal/*.conf "/home/$U/.config/xdg-desktop-portal/" 2>&1
    for f in /usr/share/xdg-desktop-portal/*.conf; do echo "--- $f"; cat "$f"; done
    echo "== backends declared"; grep -H -E '^(Interfaces|UseIn)=' /usr/share/xdg-desktop-portal/portals/*.portal
    echo "== frontend interfaces and versions"
    as_user busctl --user introspect org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop --no-pager 2>&1 | grep -E '^org\.freedesktop\.portal\.[A-Za-z]+ +interface'
    for i in ScreenCast Screenshot FileChooser Settings Secret Inhibit Notification RemoteDesktop Background GlobalShortcuts Account Print Wallpaper Camera Location InputCapture; do
        printf 'portal.%-16s version=%s\n' "$i" "$(as_user busctl --user get-property org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop org.freedesktop.portal.$i version 2>&1)"
    done
    echo "== journal of xdg-desktop-portal (which backend it chose)"
    journalctl -b --no-pager _UID="$UID_" -t xdg-desktop-portal -t xdg-desktop-por 2>/dev/null | tail -60
    as_user journalctl --user -b --no-pager -u xdg-desktop-portal.service 2>/dev/null | tail -60
} >"$OUT/portals.txt" 2>&1

# Libraries: every binary of the closure, ldd and the journal's symbol errors.
{
    for pkg in cosmic-bg cosmic-comp cosmic-greeter cosmic-greeter-daemon cosmic-idle cosmic-osd \
        cosmic-randr cosmic-settings cosmic-settings-daemon xdg-desktop-portal-cosmic; do
        dpkg -L "$pkg" | while read -r f; do
            [[ -f "$f" && -x "$f" ]] || continue
            file -b "$f" | grep -q ELF || continue
            echo "### $pkg $f"
            ldd "$f" 2>&1 | grep -E 'not found|version .* not found' || echo "all libraries found ($(ldd "$f" | wc -l) entries)"
        done
    done
} >"$OUT/ldd.txt" 2>&1
journalctl -b --no-pager >"$OUT/journal-system.txt" 2>&1
grep -E -i 'symbol lookup error|undefined symbol|version `[^`]*'"'"' not found|cannot open shared object|error while loading shared' "$OUT/journal-system.txt" >"$OUT/journal-missing-symbols.txt" || echo "none" >"$OUT/journal-missing-symbols.txt"
cp "$XDG_RUNTIME_DIR"/vm201-*.log "$XDG_RUNTIME_DIR"/vm201-*.txt "$OUT/" 2>/dev/null || true

# XWayland and DISPLAY.
{
    echo "== Xwayland processes"; ps -eo pid,ppid,user,etimes,args | grep -i [X]wayland
    echo "== DISPLAY in the user manager"; grep -E '^(DISPLAY|WAYLAND_DISPLAY)=' "$OUT/user-manager-environment.txt"
    echo "== X sockets"; ls -la /tmp/.X11-unix/
    echo "== DISPLAY in the ready program's environment"; grep -E '^(DISPLAY|WAYLAND_DISPLAY)=' "$XDG_RUNTIME_DIR/vm201-ready-env.txt" 2>/dev/null
} >"$OUT/xwayland.txt" 2>&1

# XDG autostart: what the generator made and what ran.
{
    echo "== /etc/xdg/autostart entries and their desktop filters"
    for f in /etc/xdg/autostart/*.desktop; do
        printf '%-55s %s | %s | %s\n' "$(basename "$f")" "$(grep -m1 -E '^OnlyShowIn=' "$f")" "$(grep -m1 -E '^NotShowIn=' "$f")" "$(grep -m1 -E '^X-GNOME-AutoRestart|^X-GNOME-Autostart-Phase' "$f" | tr '\n' ' ')"
    done
    echo "== generated app-*@autostart.service units and their state"
    as_user systemctl --user list-units --all --no-pager --plain 'app-*@autostart.service'
    echo "== units WantedBy graphical-session.target (installed by packages)"
    ls -l /usr/lib/systemd/user/graphical-session.target.wants/ /etc/systemd/user/graphical-session.target.wants/ "/home/$U/.config/systemd/user/graphical-session.target.wants/" 2>&1
    grep -l -E 'WantedBy=.*graphical-session.target|PartOf=graphical-session.target' /usr/lib/systemd/user/*.service 2>/dev/null
} >"$OUT/autostart.txt" 2>&1
echo "inventory written to $OUT"
