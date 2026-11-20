# shellcheck shell=sh disable=SC2154 # build-deb.sh writes the header that sets the better_os_* variables
# Removes the autostart overrides Better Touchpad wrote (ticket 88), when the
# package is removed. Turning off a competing gesture tool (Touchégg, fusuma)
# writes a Hidden=true copy of its autostart entry into the person's
# ~/.config/autostart, marked with one line; nothing the component added may
# outlive it (AGENTS.md), so removal deletes exactly those files.
#
# Who is a person, the time limit on each person's step, and the account
# switch are accounts.sh's and account-step.sh's, which the generated script
# puts in the same library.
#
# Header variables set by the generated script, besides accounts.sh's:
#   better_os_override_mark the exact marker line
#   better_os_override_files space-separated file names in autostart
#
# A home that is not absolute, or is "/", is refused. Neither ~/.config nor
# ~/.config/autostart may be a symbolic link, and both must be directories
# owned by the account. Only a regular file (never a symbolic link) owned by
# that account and carrying the exact marker line is removed, and also the
# page's own temporary copy of one (`<name>.tmp-better-os`) that a crash left
# behind, under the same rule.
#
# The last check and the removal run as that account (setpriv), never as
# root: whatever a link swapped in between the checks points at, the delete
# can only do what the account itself could, so it can never reach a file
# under /etc/xdg/autostart or anyone else's home. A file the person wrote has
# no marker and is left alone. A custom XDG_CONFIG_HOME is not visible from a
# maintainer script; such an override stays, and the page can still remove it
# ("Turn back on").

better_os_autostart_step() {
    # $1 name, $2 uid, $3 gid, $4 home; runs in its own timed shell.
    uid="$2"
    gid="$3"
    home="$4"
    case "$home" in
        /*) ;;
        *) return 0 ;;
    esac
    [ -n "$(printf '%s' "$home" | tr -d /)" ] || return 0
    better_os_owned_real_dir "$home/.config" "$uid" || return 0
    better_os_owned_real_dir "$home/.config/autostart" "$uid" || return 0
    for file in $better_os_override_files; do
        for path in "$home/.config/autostart/$file" "$home/.config/autostart/$file.tmp-better-os"; do
            # shellcheck disable=SC2015 # both tests pass or the path is skipped
            [ -f "$path" ] && [ ! -L "$path" ] || continue
            [ "$(stat -c %u -- "$path" 2>/dev/null)" = "$uid" ] || continue
            # shellcheck disable=SC2016 # expanded by the inner shell, not this one
            better_os_as_account "$uid" "$gid" sh -c \
                '[ -f "$1" ] && [ ! -L "$1" ] && grep -qxF "$2" "$1" && rm -f -- "$1"' \
                better-os-autostart "$path" "$better_os_override_mark" || true
        done
    done
    return 0
}

better_os_forget_autostart_overrides() {
    case "${1:-}" in
        remove | purge) better_os_for_each_account better_os_autostart_step ;;
    esac
    return 0
}
