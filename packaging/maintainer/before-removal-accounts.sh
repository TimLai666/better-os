# shellcheck shell=sh disable=SC2154 # build-deb.sh writes the header that sets the better_os_* variables
# Runs a package's before-removal program as each person when apt or dpkg
# removes the package (ticket 90), for a package Better Manager cannot
# remove itself: Better Manager is that package. Better Manager runs a
# component's program in the person's session when it removes the
# component; this is the same program, run by the maintainer scripts, once
# per account, as that account.
#
#   prerm remove           runs the program, which undoes what the package
#                          changed in the person's files and sets aside what
#                          it replaced
#   postinst abort-remove  runs it with --undo, because the removal did not
#                          happen
#   postrm remove|purge    deletes what the program set aside, as the account
#
# Needs accounts.sh, which the generated script includes first.
#
# Header variables set by the generated script:
#   better_os_passwd_file             the passwd database to read homes from
#   better_os_before_removal_program  the program, by absolute path
#   better_os_before_removal_leftover what it sets aside, relative to the home
#
# The program runs with a cleared environment holding only HOME, a fixed
# PATH, and LC_ALL=C, so it finds the person's files under their home and
# nothing a root environment carries. A custom XDG_DATA_HOME or
# XDG_STATE_HOME is not visible from a maintainer script; files there stay.
# Its exit status never stops the removal: 3 means not everything could be
# undone, and what it says goes to the terminal apt runs in.

better_os_before_removal_each() {
    # $1: what to run for each account, as "run" (the program), "undo" (the
    # program with --undo), or "forget" (delete the leftover).
    [ -r "$better_os_passwd_file" ] || return 0
    uid_min="$(better_os_login_range_value UID_MIN 1000)"
    uid_max="$(better_os_login_range_value UID_MAX 60000)"
    while IFS=: read -r _name _pw uid gid _gecos home _shell; do
        case "$uid" in
            '' | *[!0-9]*) continue ;;
        esac
        # shellcheck disable=SC2015 # both tests pass or the account is skipped
        [ "$uid" -ge "$uid_min" ] && [ "$uid" -le "$uid_max" ] || continue
        better_os_usable_home "$home" "$uid" || continue
        case "$1" in
            run | undo)
                [ -x "$better_os_before_removal_program" ] || return 0
                if [ "$1" = undo ]; then
                    better_os_as_account "$uid" "$gid" env -i HOME="$home" \
                        PATH=/usr/bin:/bin LC_ALL=C \
                        "$better_os_before_removal_program" --undo || true
                else
                    better_os_as_account "$uid" "$gid" env -i HOME="$home" \
                        PATH=/usr/bin:/bin LC_ALL=C \
                        "$better_os_before_removal_program" || true
                fi
                ;;
            forget)
                # shellcheck disable=SC2016 # expanded by the inner shell, not this one
                better_os_as_account "$uid" "$gid" sh -c \
                    'p="$1/$2"; [ -f "$p" ] && [ ! -L "$p" ] && rm -f -- "$p"; exit 0' \
                    better-os-forget "$home" "$better_os_before_removal_leftover" || true
                ;;
        esac
    done < "$better_os_passwd_file"
    return 0
}

better_os_before_removal_prerm() {
    case "${1:-}" in
        remove) better_os_before_removal_each run ;;
    esac
    return 0
}

better_os_before_removal_postinst() {
    case "${1:-}" in
        abort-remove) better_os_before_removal_each undo ;;
    esac
    return 0
}

better_os_before_removal_postrm() {
    case "${1:-}" in
        remove | purge) better_os_before_removal_each forget ;;
    esac
    return 0
}
