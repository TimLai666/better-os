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
# Who is a person, the time limit on each person's step and on the whole
# walk, and the account switch are accounts.sh's and account-step.sh's, which
# the generated postrm puts in the same library (build-deb.sh account_block).
#
# Header variables set by the generated script, besides accounts.sh's:
#   better_os_activation_name  the bus name (org.freedesktop.FileManager1)
#   better_os_activation_exec  the exact Exec= line Better Defaults writes
#
# Root checks only that the home is the account's own directory
# (better_os_usable_home). Everything inside the home is looked at, read and
# changed by the account itself, in a new session with standard input from
# /dev/null and a cleared environment, so whatever is swapped in between two
# checks can only lead to what the account could do anyway. As that account:
#
#   - every directory from ~/.local to ~/.local/share/dbus-1/services must be
#     a real directory, not a link;
#   - the activation file is read only when it is a regular file the account
#     owns; a link, a FIFO, a device, or anything else is never opened, and
#     the step's time limit bounds a swap between the test and the read;
#   - it is deleted only when it is byte for byte the one Better Defaults
#     writes; a file the person wrote is theirs and stays;
#   - the person's own file Better Defaults kept aside
#     (.<name>.service.before-better-os) is renamed back when nothing is left
#     in its place, as Better Manager's own leftover pass does
#     (defaults_platform::bus_activation::remove_own_files);
#   - directories the removal left empty go, and a running session bus is
#     asked to read its service directories again.
#
# Nothing here runs on upgrade, and nothing here fails the removal.

better_os_activation_step() {
    # $1 name, $2 uid, $3 gid, $4 home; runs in its own timed shell.
    name="$1"
    uid="$2"
    gid="$3"
    home="$4"
    better_os_usable_home "$home" "$uid" || return 0
    # shellcheck disable=SC2016 # expanded by the account's own shell, not this one
    better_os_as_account "$uid" "$gid" setsid env -i PATH=/usr/bin:/bin LC_ALL=C sh -c '
        home="$1"
        name="$2"
        exec_line="$3"
        bus="$4"
        services="$home/.local/share/dbus-1/services"
        for dir in "$home/.local" "$home/.local/share" "$home/.local/share/dbus-1" "$services"; do
            if [ ! -d "$dir" ] || [ -L "$dir" ]; then
                exit 0
            fi
        done
        file="$services/$name.service"
        aside="$services/.$name.service.before-better-os"
        changed=no
        if [ -f "$file" ] && [ ! -L "$file" ] && [ -O "$file" ] &&
            printf "[D-BUS Service]\nName=%s\nExec=%s\n" "$name" "$exec_line" | cmp -s - "$file"; then
            rm -f -- "$file" && changed=yes
        fi
        if [ ! -e "$file" ] && [ ! -L "$file" ] &&
            [ -f "$aside" ] && [ ! -L "$aside" ] && [ -O "$aside" ]; then
            mv -f -- "$aside" "$file" && changed=yes && echo "your own $name.service is back in use"
        fi
        [ "$changed" = yes ] || exit 0
        rmdir -- "$services" 2>/dev/null && rmdir -- "${services%/*}" 2>/dev/null
        if [ -S "$bus" ]; then
            dbus-send --bus="unix:path=$bus" --type=method_call \
                --dest=org.freedesktop.DBus / org.freedesktop.DBus.ReloadConfig \
                > /dev/null 2>&1
        fi
        exit 0
    ' better-os-activation "$home" "$better_os_activation_name" "$better_os_activation_exec" \
        "${better_os_session_root:-/run/user}/$uid/bus" < /dev/null 2>&1 |
        better_os_prefix_lines "$name" || true
    return 0
}

better_os_forget_activation_files() {
    case "${1:-}" in
        remove | purge) better_os_for_each_account better_os_activation_step ;;
    esac
    return 0
}
