# shellcheck shell=sh disable=SC2154 # build-deb.sh writes the header that sets the better_os_* variables
# The people a maintainer script acts for, shared by every fragment that
# changes something in a person's home on removal (autostart-overrides.sh,
# before-removal-accounts.sh): one reading of the login range and one way to
# reach each person, so the two never disagree about who is a person.
#
# The generated script writes this file, account-step.sh, and the fragment
# into one private temporary library ($better_os_account_lib), sources it,
# and removes it at the end. Each person's step runs in its own `sh` that
# sources the same library, under `timeout -s KILL`: a home on a dead NFS
# server or a hung FUSE mount can stop only that person's step, never the
# removal, and dpkg's lock is let go. Root's own checks of the home run
# inside the step too, for the same reason.
#
# Header variables set by the generated script:
#   better_os_login_defs    login.defs, for the UID_MIN..UID_MAX login range
#   better_os_getent        the program that lists accounts (getent, which
#                           also reaches LDAP, sssd, and systemd-homed
#                           accounts; a test passes a stand-in)
#   better_os_setpriv       the program that runs a command as an account
#                           (util-linux setpriv; a test passes a stand-in)
#   better_os_step_seconds  how long one person's step may take (30)
#   better_os_account_lib   the library file itself
#
# Accounts in the login range (UID_MIN..UID_MAX from login.defs, 1000 and
# 60000 when it says nothing) are people; nobody (65534) and system accounts
# are not.

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

better_os_for_each_account() {
    # $1: the step function, then its own arguments; it is called as
    # `step <args> <name> <uid> <gid> <home>` in its own timed `sh`.
    seconds="${better_os_step_seconds:-30}"
    uid_min="$(better_os_login_range_value UID_MIN 1000)"
    uid_max="$(better_os_login_range_value UID_MAX 60000)"
    # A directory service that does not answer stops the listing, not the
    # removal.
    accounts="$(timeout -s KILL "$seconds" "${better_os_getent:-getent}" passwd 2>/dev/null)" || true
    printf '%s\n' "$accounts" | while IFS=: read -r name _pw uid gid _gecos home _shell; do
        case "$uid" in
            '' | *[!0-9]*) continue ;;
        esac
        # shellcheck disable=SC2015 # both tests pass or the account is skipped
        [ "$uid" -ge "$uid_min" ] && [ "$uid" -le "$uid_max" ] || continue
        status=0
        # shellcheck disable=SC2016 # expanded by the step's own shell
        timeout -s KILL "$seconds" sh -c '. "$1"; shift; "$@"' better-os-step \
            "$better_os_account_lib" "$@" "$name" "$uid" "$gid" "$home" \
            < /dev/null || status=$?
        case "$status" in
            124 | 137)
                printf 'Better OS: %s (%s) did not answer within %s seconds; its step was skipped\n' \
                    "$name" "$home" "$seconds" >&2
                ;;
        esac
    done
    return 0
}
