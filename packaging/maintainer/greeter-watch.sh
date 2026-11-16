# The login screen watch's enablement in the daemon package's maintainer
# scripts (ADR 0033, ticket 140), in debhelper's own shape: enabled on the
# first install, and on an upgrade only when it was enabled before, so an
# administrator who disabled it keeps it disabled.

better_os_greeter_watch_unit='better-greeter-watch.service'

# postinst configure. $1 is the previously configured version, empty on a
# first install.
better_os_greeter_watch_configure() {
    if command -v deb-systemd-helper >/dev/null 2>&1; then
        if deb-systemd-helper debian-installed "$better_os_greeter_watch_unit"; then
            if deb-systemd-helper --quiet was-enabled "$better_os_greeter_watch_unit"; then
                deb-systemd-helper enable "$better_os_greeter_watch_unit" >/dev/null 2>&1 || true
            else
                deb-systemd-helper update-state "$better_os_greeter_watch_unit" >/dev/null 2>&1 || true
            fi
        else
            deb-systemd-helper enable "$better_os_greeter_watch_unit" >/dev/null 2>&1 || true
        fi
    elif [ -z "${1:-}" ] && [ -d /run/systemd/system ]; then
        systemctl enable "$better_os_greeter_watch_unit" >/dev/null 2>&1 || true
    fi
}

# prerm remove.
better_os_greeter_watch_disable() {
    if command -v deb-systemd-helper >/dev/null 2>&1; then
        deb-systemd-helper disable "$better_os_greeter_watch_unit" >/dev/null 2>&1 || true
    elif [ -d /run/systemd/system ]; then
        systemctl disable "$better_os_greeter_watch_unit" >/dev/null 2>&1 || true
    fi
}
