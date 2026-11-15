# shellcheck shell=sh disable=SC2154 # build-deb.sh writes the header that sets the better_os_scroll_* variables
# The system half of better-touchpad: the scroll pass-through (ADR 0030),
# one system service per touchpad, started by udev.
#
# Sourced into better-touchpad's postinst, prerm, and postrm after a header
# that sets:
#   better_os_scroll_libexec   the service binary
#   better_os_scroll_state     its state directory
#   better_os_scroll_user      its account
# Every command is looked up on PATH, so the test drives it against fakes.
#
# Installing is what turns it on: postinst creates the account and the state
# directory, reloads the bus, and asks systemd to run
# better-touchpad-scroll-coldplug.service, which triggers udev for the
# touchpads already there. That unit runs in PID 1's context, so it works even
# when this script runs inside Better Manager's daemon sandbox, where /sys is
# read-only. If it still fails, the script writes
# $better_os_scroll_reconnect, which Better Touchpad's Scrolling page reads to
# say "restart or reconnect the touchpad"; the person never depends on this
# script's output. udev starts an instance for every touchpad plugged in later.
# Removing stops every instance and deletes the account and the state; dpkg
# removes the units, the rule, and the bus policy. A grab or a clone never
# outlives its process, so stopping the instances is all it takes to give
# every touchpad back.

better_os_scroll_unit_glob='better-touchpad-scroll@*.service'

better_os_scroll_systemd_running() {
    [ -d "${better_os_scroll_systemd_dir:-/run/systemd/system}" ]
}

# postinst configure [old-version]
better_os_scroll_postinst() {
    [ "$1" = configure ] || return 0
    systemd-sysusers better-touchpad-scroll.conf >/dev/null 2>&1 || true
    systemd-tmpfiles --create better-touchpad-scroll.conf >/dev/null 2>&1 || true
    # dpkg unpacked the bus policy before the account existed, so the bus read
    # a policy naming an unknown user and dropped its own rule. Reload it now
    # that the account is there, or no instance could own its name.
    dbus-send --system --type=method_call --dest=org.freedesktop.DBus \
        /org/freedesktop/DBus org.freedesktop.DBus.ReloadConfig >/dev/null 2>&1 || true
    better_os_scroll_systemd_running || return 0
    systemctl daemon-reload >/dev/null 2>&1 || true
    udevadm control --reload-rules >/dev/null 2>&1 || true
    if systemctl start better-touchpad-scroll-coldplug.service >/dev/null 2>&1; then
        rm -f -- "$better_os_scroll_reconnect"
    else
        mkdir -p -- "${better_os_scroll_reconnect%/*}" 2>/dev/null || true
        : > "$better_os_scroll_reconnect" 2>/dev/null || true
    fi
    if [ -n "${2:-}" ]; then
        # An upgrade: the running instances restart on the new program.
        systemctl try-restart "$better_os_scroll_unit_glob" >/dev/null 2>&1 || true
    fi
    for event in $("$better_os_scroll_libexec" --eligible 2>/dev/null); do
        case "$event" in
            event[0-9]*) systemctl start --no-block "better-touchpad-scroll@$event.service" \
                >/dev/null 2>&1 || true ;;
        esac
    done
}

# prerm remove|upgrade|deconfigure|failed-upgrade
better_os_scroll_prerm() {
    case "$1" in
        remove | deconfigure)
            better_os_scroll_systemd_running || return 0
            systemctl stop "$better_os_scroll_unit_glob" >/dev/null 2>&1 || true
            ;;
    esac
}

# postrm remove|purge|upgrade|...
better_os_scroll_postrm() {
    case "$1" in
        remove | purge)
            if better_os_scroll_systemd_running; then
                udevadm control --reload-rules >/dev/null 2>&1 || true
                systemctl daemon-reload >/dev/null 2>&1 || true
                systemctl reset-failed "$better_os_scroll_unit_glob" >/dev/null 2>&1 || true
            fi
            rm -rf -- "$better_os_scroll_state"
            rm -f -- "$better_os_scroll_reconnect"
            # An instance udev started between prerm and now would keep the
            # account in use and make userdel fail: stop them all again.
            if better_os_scroll_systemd_running; then
                systemctl stop "$better_os_scroll_unit_glob" >/dev/null 2>&1 || true
            fi
            if getent passwd "$better_os_scroll_user" >/dev/null 2>&1; then
                if ! userdel "$better_os_scroll_user" >/dev/null 2>&1; then
                    echo "better-touchpad: could not remove the account $better_os_scroll_user; remove it with: sudo userdel $better_os_scroll_user" >&2
                fi
            fi
            if getent group "$better_os_scroll_user" >/dev/null 2>&1; then
                groupdel "$better_os_scroll_user" >/dev/null 2>&1 || true
            fi
            ;;
    esac
}
