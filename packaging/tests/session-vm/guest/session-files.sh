#!/usr/bin/env bash
# Run as root inside a disposable VM. Writes a hand-made stand-in for the
# session package ticket 203 builds: a session file GDM lists, a launcher that
# sets the session's environment and runs cosmic-comp, a "ready" program that
# cosmic-comp runs once it is up, a user target bound to
# graphical-session.target, the betteros dconf profile and portal order, and a
# Fcitx 5 profile with Chewing for one person. No Better OS code.
#
#   session-files.sh USER
#
# remove-session-files.sh takes all of it out again.
set -euo pipefail
U="$1"
H="$(getent passwd "$U" | cut -d: -f6)"
L=/usr/local/libexec/vm201

install -d "$L"
cat >/usr/share/wayland-sessions/better-os.desktop <<EOF
[Desktop Entry]
Name=Better OS
Comment=Better OS session on cosmic-comp (ticket 201 stand-in)
Exec=$L/session-start
TryExec=cosmic-comp
Type=Application
DesktopNames=BetterOS;COSMIC
EOF

cat >"$L/session-start" <<EOF
#!/bin/sh
# Environment of better-os-session.md, "The environment the launcher sets".
export XDG_CURRENT_DESKTOP=BetterOS:COSMIC
export XDG_SESSION_DESKTOP=better-os DESKTOP_SESSION=better-os
export DCONF_PROFILE=betteros
export GTK_IM_MODULE=fcitx QT_IM_MODULE=fcitx XMODIFIERS=@im=fcitx
exec > "\${XDG_RUNTIME_DIR:-/tmp}/vm201-session.log" 2>&1
echo "session-start \$(date -Is) XDG_SESSION_TYPE=\$XDG_SESSION_TYPE"
exec cosmic-comp $L/session-ready
EOF

cat >"$L/session-ready" <<'EOF'
#!/bin/sh
# Run by cosmic-comp once it is up.
VARS="WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP DESKTOP_SESSION DCONF_PROFILE GTK_IM_MODULE QT_IM_MODULE XMODIFIERS"
echo "ready $(date -Is) WAYLAND_DISPLAY=$WAYLAND_DISPLAY DISPLAY=${DISPLAY-<unset>}"
env | sort > "$XDG_RUNTIME_DIR/vm201-ready-env.txt"
systemctl --user import-environment $VARS
dbus-update-activation-environment --systemd $VARS
systemctl --user start vm201-session.target
# cosmic-greeter in a user session is the locker: it answers logind's Lock for
# the session it finds by its own PID, so it runs here, inside the session's
# scope, and not as a user unit (logind: NoSessionForPID).
for p in cosmic-settings-daemon cosmic-osd cosmic-idle cosmic-bg cosmic-greeter; do
    "$p" > "$XDG_RUNTIME_DIR/vm201-$p.log" 2>&1 &
done
fcitx5 -d --replace > "$XDG_RUNTIME_DIR/vm201-fcitx5.log" 2>&1
gnome-terminal > "$XDG_RUNTIME_DIR/vm201-terminal.log" 2>&1 &
# Stay while the session lasts; on exit stop the target and unset what was
# imported, as the session launcher will.
trap 'systemctl --user stop vm201-session.target; systemctl --user unset-environment $VARS; exit 0' TERM INT HUP
while :; do sleep 3600 & wait $!; done
EOF
chmod 0755 "$L/session-start" "$L/session-ready"

cat >/usr/lib/systemd/user/vm201-session.target <<'EOF'
[Unit]
Description=Better OS session stand-in (ticket 201)
BindsTo=graphical-session.target
Wants=graphical-session-pre.target
After=graphical-session-pre.target
Wants=xdg-desktop-autostart.target
Before=xdg-desktop-autostart.target
EOF

install -d /usr/share/dconf/profile
printf 'user-db:betteros\nuser-db:user\n' >/usr/share/dconf/profile/betteros
install -d /usr/share/xdg-desktop-portal
# COSMIC's own cosmic-portals.conf names the Secret backend too: without that
# line no Secret portal is offered (gnome-keyring.portal is UseIn=gnome).
printf '[preferred]\ndefault=cosmic;gtk;\norg.freedesktop.impl.portal.Secret=gnome-keyring;\n' \
    >/usr/share/xdg-desktop-portal/betteros-portals.conf

# Fcitx 5 with Chewing for the test person (the carry-over is ticket 208's).
install -d -o "$U" -g "$U" "$H/.config" "$H/.config/fcitx5"
cat >"$H/.config/fcitx5/profile" <<'EOF'
[Groups/0]
Name=Default
Default Layout=us
DefaultIM=chewing

[Groups/0/Items/0]
Name=keyboard-us
Layout=

[Groups/0/Items/1]
Name=chewing
Layout=

[GroupOrder]
0=Default
EOF
chown -R "$U:$U" "$H/.config/fcitx5"
echo "session files written for $U"
