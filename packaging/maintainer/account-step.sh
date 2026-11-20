# shellcheck shell=sh disable=SC2154 # build-deb.sh writes the header that sets the better_os_* variables
# What one person's step may use, run inside its own timed `sh` (see
# accounts.sh): whether a home is usable, and acting as the account.
#
# A home must be absolute, not "/", a real directory (not a link) owned by
# the account. An unmounted encrypted home is an empty directory owned by
# root, or not there, so it is skipped: there is nothing of the person's to
# reach until they log in.

better_os_owned_real_dir() {
    # $1 path, $2 uid: a directory, not a link, owned by uid.
    [ -d "$1" ] && [ ! -L "$1" ] || return 1
    [ "$(stat -c %u -- "$1" 2>/dev/null)" = "$2" ]
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
