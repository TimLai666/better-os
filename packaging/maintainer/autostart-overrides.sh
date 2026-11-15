# shellcheck shell=sh
# Removes the autostart overrides Better Touchpad wrote (ticket 88), when the
# package is removed. Turning off a competing gesture tool (Touchégg, fusuma)
# writes a Hidden=true copy of its autostart entry into the person's
# ~/.config/autostart, marked with one line; nothing the component added may
# outlive it (AGENTS.md), so removal deletes exactly those files.
#
# Header variables set by the generated script:
#   better_os_passwd_file   the passwd database to read homes from
#   better_os_login_defs    login.defs, for the UID_MIN..UID_MAX login range
#   better_os_override_mark the exact marker line
#   better_os_override_files space-separated file names in autostart
#
# Only accounts in the login range (UID_MIN..UID_MAX from login.defs, 1000 and
# 60000 when it says nothing) are read, so nobody (65534) and system accounts
# are skipped. A home that is not absolute, or is "/", is refused. Neither
# ~/.config nor ~/.config/autostart may be a symbolic link, and both must be
# directories owned by the account. Only a regular file (never a symbolic
# link) owned by that account and carrying the exact marker line is removed. A
# file the person wrote has no marker and is left alone. A custom
# XDG_CONFIG_HOME is not visible from a maintainer script; such an override
# stays, and the page can still remove it ("Turn back on").

better_os_login_range_value() {
    # $1 key, $2 default; a value that is not a plain number is ignored.
    value=""
    if [ -r "${better_os_login_defs:-}" ]; then
        value="$(awk -v key="$1" '$1 == key { v = $2 } END { print v }' "$better_os_login_defs" 2>/dev/null)" || value=""
    fi
    case "$value" in
        '' | *[!0-9]*) value="$2" ;;
    esac
    printf '%s\n' "$value"
}

better_os_owned_real_dir() {
    # $1 path, $2 uid: a directory, not a link, owned by uid.
    [ -d "$1" ] && [ ! -L "$1" ] || return 1
    [ "$(stat -c %u -- "$1" 2>/dev/null)" = "$2" ]
}

better_os_forget_autostart_overrides() {
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
        [ "$uid" -ge "$uid_min" ] && [ "$uid" -le "$uid_max" ] || continue
        case "$home" in
            /*) ;;
            *) continue ;;
        esac
        [ -n "$(printf '%s' "$home" | tr -d /)" ] || continue
        better_os_owned_real_dir "$home/.config" "$uid" || continue
        better_os_owned_real_dir "$home/.config/autostart" "$uid" || continue
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
