# shellcheck shell=sh disable=SC2154 # build-deb.sh writes the header that sets the better_os_* variables
# The people a maintainer script acts for, shared by every fragment that
# changes something in a person's home on removal (autostart-overrides.sh,
# before-removal-accounts.sh): one reading of the login range and one way to
# act as an account, so the two never disagree about who is a person.
#
# Header variables set by the generated script:
#   better_os_login_defs    login.defs, for the UID_MIN..UID_MAX login range
#   better_os_setpriv       the program that runs a command as an account
#                           (util-linux setpriv; a test passes a stand-in)
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

better_os_owned_real_dir() {
    # $1 path, $2 uid: a directory, not a link, owned by uid.
    [ -d "$1" ] && [ ! -L "$1" ] || return 1
    [ "$(stat -c %u -- "$1" 2>/dev/null)" = "$2" ]
}

better_os_as_account() {
    # $1 uid, $2 gid, then the command. Already that account (a test, or a
    # person removing their own copy): run it as is.
    as_uid="$1"
    as_gid="$2"
    shift 2
    case "$as_gid" in
        '' | *[!0-9]*) return 1 ;;
    esac
    if [ "$(id -u)" = "$as_uid" ]; then
        "$@"
    else
        "${better_os_setpriv:-setpriv}" --reuid="$as_uid" --regid="$as_gid" --init-groups -- "$@"
    fi
}

better_os_usable_home() {
    # $1 home, $2 uid: absolute, not "/", a real directory the account owns.
    case "$1" in
        /*) ;;
        *) return 1 ;;
    esac
    [ -n "$(printf '%s' "$1" | tr -d /)" ] || return 1
    better_os_owned_real_dir "$1" "$2"
}
