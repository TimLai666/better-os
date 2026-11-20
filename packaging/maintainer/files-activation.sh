# shellcheck shell=sh disable=SC2154 # build-deb.sh writes the header that sets the better_os_* variables
# Removes the user-level D-Bus activation file Better Defaults wrote for
# Better Files (ticket 129, ADR 0041), when the package is removed by any
# means: `apt remove`, Software, unattended upgrades. The file in a person's
# ~/.local/share/dbus-1/services outranks Nautilus' own, so one left behind
# would point "Show in folder" at a program that is gone. Better Manager's own
# removal puts the previous file back first (the before-removal program);
# this is for every removal that never ran it, and for every account, logged
# in or not.
#
# It reads the helpers of packaging/maintainer/autostart-overrides.sh, which
# the generated postrm carries before it: the login range, a directory that
# is the account's own, and running the last check and the removal as that
# account. Header variables set by the generated script, besides those:
#   better_os_activation_name  the bus name (org.freedesktop.FileManager1)
#   better_os_activation_exec  the exact Exec= line Better Defaults writes
#   better_os_run_user_root    /run/user, where a logged-in person's bus is
#
# Only a regular file owned by the account and byte for byte the one Better
# Defaults writes is removed: a file the person wrote is theirs and stays.
# Afterwards each logged-in person's bus is asked to read its service
# directories again. Nothing here fails the removal.

better_os_expected_activation() {
    printf '[D-BUS Service]\nName=%s\nExec=%s\n' \
        "$better_os_activation_name" "$better_os_activation_exec"
}

better_os_forget_activation_files() {
    case "${1:-}" in
        remove | purge) ;;
        *) return 0 ;;
    esac
    [ -r "$better_os_passwd_file" ] || return 0
    uid_min="$(better_os_login_range_value UID_MIN 1000)"
    uid_max="$(better_os_login_range_value UID_MAX 60000)"
    while IFS=: read -r _name _pw uid _gid _gecos home _shell; do
        case "$uid" in
            '' | *[!0-9]*) continue ;;
        esac
        # shellcheck disable=SC2015 # both tests pass or the account is skipped
        [ "$uid" -ge "$uid_min" ] && [ "$uid" -le "$uid_max" ] || continue
        case "$home" in
            /*) ;;
            *) continue ;;
        esac
        [ -n "$(printf '%s' "$home" | tr -d /)" ] || continue
        services="$home/.local/share/dbus-1/services"
        better_os_owned_real_dir "$home/.local" "$uid" || continue
        better_os_owned_real_dir "$home/.local/share" "$uid" || continue
        better_os_owned_real_dir "$home/.local/share/dbus-1" "$uid" || continue
        better_os_owned_real_dir "$services" "$uid" || continue
        path="$services/$better_os_activation_name.service"
        # shellcheck disable=SC2015 # both tests pass or the path is skipped
        [ -f "$path" ] && [ ! -L "$path" ] || continue
        [ "$(stat -c %u -- "$path" 2>/dev/null)" = "$uid" ] || continue
        better_os_expected_activation | cmp -s - "$path" || continue
        # shellcheck disable=SC2016 # expanded by the inner shell, not this one
        better_os_as_account "$uid" "$_gid" sh -c \
            '[ -f "$1" ] && [ ! -L "$1" ] && printf "[D-BUS Service]\nName=%s\nExec=%s\n" "$2" "$3" | cmp -s - "$1" && rm -f -- "$1" && rmdir -- "${1%/*}" 2>/dev/null; true' \
            better-os-activation "$path" "$better_os_activation_name" \
            "$better_os_activation_exec" || true
        bus="$better_os_run_user_root/$uid/bus"
        if [ -S "$bus" ]; then
            better_os_as_account "$uid" "$_gid" dbus-send \
                --bus="unix:path=$bus" --type=method_call \
                --dest=org.freedesktop.DBus / org.freedesktop.DBus.ReloadConfig \
                >/dev/null 2>&1 || true
        fi
    done < "$better_os_passwd_file"
    return 0
}
