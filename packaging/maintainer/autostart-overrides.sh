# shellcheck shell=sh
# Removes the autostart overrides Better Touchpad wrote (ticket 88), when the
# package is removed. Turning off a competing gesture tool (Touchégg, fusuma)
# writes a Hidden=true copy of its autostart entry into the person's
# ~/.config/autostart, marked with one line; nothing the component added may
# outlive it (AGENTS.md), so removal deletes exactly those files.
#
# Header variables set by the generated script:
#   better_os_passwd_file   the passwd database to read homes from
#   better_os_override_mark the exact marker line
#   better_os_override_files space-separated file names in autostart
#
# Only a regular file (never a symbolic link) owned by that account, inside
# that account's own home, and carrying the exact marker line is removed. A
# file the person wrote has no marker and is left alone. A custom
# XDG_CONFIG_HOME is not visible from a maintainer script; such an override
# stays, and the page can still remove it ("Turn back on").

better_os_forget_autostart_overrides() {
    case "${1:-}" in
        remove | purge) ;;
        *) return 0 ;;
    esac
    [ -r "$better_os_passwd_file" ] || return 0
    while IFS=: read -r _name _pw uid _gid _gecos home _shell; do
        case "$uid" in
            '' | *[!0-9]*) continue ;;
        esac
        [ "$uid" -ge 1000 ] || continue
        [ -n "$home" ] && [ -d "$home/.config/autostart" ] || continue
        for file in $better_os_override_files; do
            path="$home/.config/autostart/$file"
            [ -f "$path" ] && [ ! -L "$path" ] || continue
            [ "$(stat -c %u -- "$path" 2>/dev/null)" = "$uid" ] || continue
            grep -qxF "$better_os_override_mark" "$path" 2>/dev/null || continue
            rm -f -- "$path" || true
        done
    done < "$better_os_passwd_file"
    return 0
}
