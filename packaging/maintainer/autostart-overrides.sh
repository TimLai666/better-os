# shellcheck shell=sh disable=SC2154 # build-deb.sh writes the header that sets the better_os_* variables
# Removes the autostart overrides Better Touchpad wrote (ticket 88), when the
# package is removed. Turning off a competing gesture tool (Touchégg, fusuma)
# writes a Hidden=true copy of its autostart entry into the person's
# ~/.config/autostart, marked with one line; nothing the component added may
# outlive it (AGENTS.md), so removal deletes exactly those files.
#
# The login range and the account switch are accounts.sh's, which the
# generated script includes first.
#
# Header variables set by the generated script:
#   better_os_passwd_file   the passwd database to read homes from
#   better_os_login_defs    login.defs, for the UID_MIN..UID_MAX login range
#   better_os_override_mark the exact marker line
#   better_os_override_files space-separated file names in autostart
#   better_os_setpriv       the program that runs a command as an account
#                           (util-linux setpriv; a test passes a stand-in)
#
# Only accounts in the login range (UID_MIN..UID_MAX from login.defs, 1000 and
# 60000 when it says nothing) are read, so nobody (65534) and system accounts
# are skipped. A home that is not absolute, or is "/", is refused. Neither
# ~/.config nor ~/.config/autostart may be a symbolic link, and both must be
# directories owned by the account. Only a regular file (never a symbolic
# link) owned by that account and carrying the exact marker line is removed,
# and also the page's own temporary copy of one (`<name>.tmp-better-os`) that
# a crash left behind, under the same rule.
#
# The last check and the removal run as that account (setpriv), never as
# root: whatever a link swapped in between the checks points at, the delete
# can only do what the account itself could, so it can never reach a file
# under /etc/xdg/autostart or anyone else's home. A
# file the person wrote has no marker and is left alone. A custom
# XDG_CONFIG_HOME is not visible from a maintainer script; such an override
# stays, and the page can still remove it ("Turn back on").

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
        # shellcheck disable=SC2015 # both tests pass or the account is skipped
        [ "$uid" -ge "$uid_min" ] && [ "$uid" -le "$uid_max" ] || continue
        case "$home" in
            /*) ;;
            *) continue ;;
        esac
        [ -n "$(printf '%s' "$home" | tr -d /)" ] || continue
        better_os_owned_real_dir "$home/.config" "$uid" || continue
        better_os_owned_real_dir "$home/.config/autostart" "$uid" || continue
        for file in $better_os_override_files; do
            for path in "$home/.config/autostart/$file" "$home/.config/autostart/$file.tmp-better-os"; do
                # shellcheck disable=SC2015 # both tests pass or the path is skipped
                [ -f "$path" ] && [ ! -L "$path" ] || continue
                [ "$(stat -c %u -- "$path" 2>/dev/null)" = "$uid" ] || continue
                # shellcheck disable=SC2016 # expanded by the inner shell, not this one
                better_os_as_account "$uid" "$_gid" sh -c \
                    '[ -f "$1" ] && [ ! -L "$1" ] && grep -qxF "$2" "$1" && rm -f -- "$1"' \
                    better-os-autostart "$path" "$better_os_override_mark" || true
            done
        done
    done < "$better_os_passwd_file"
    return 0
}
