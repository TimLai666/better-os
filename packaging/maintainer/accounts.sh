# shellcheck shell=sh disable=SC2154 # build-deb.sh writes the header that sets the better_os_* variables
# Acting for each person from a maintainer script: run a small step as each
# account, safely. Shared by every fragment that changes something in a
# person's home on removal (autostart-overrides.sh,
# before-removal-accounts.sh), so they never disagree about who is a person.
#
# How a script uses it (build-deb.sh's account_block writes this for you):
#
#   The library is the text of this file, account-step.sh, and the fragment,
#   after the header assignments below, held in the variable
#   $better_os_account_library. No file is written: the script evals the text
#   once, and each person's step runs as `sh -c "<library>" ...`, so nothing
#   in /tmp or $TMPDIR can be swapped for root to run, and a missing or full
#   temporary directory cannot fail the script. The block is only built for
#   the maintainer-script actions that need it, and nothing in it can fail
#   the script.
#
#   better_os_for_each_account STEP [ARG...]
#       Calls `STEP ARG... NAME UID GID HOME` once for each person, each in a
#       new `sh` with the whole library defined, under
#       `timeout -s KILL $better_os_step_seconds`, with standard input from
#       /dev/null. Returns 0 whatever the steps do.
#       The step may assume:
#         - NAME matches [A-Za-z0-9._-]+ and does not start with `-`; an
#           account with any other name is skipped with a line saying so;
#         - UID and GID are numbers, UID in the login range;
#         - HOME is absolute, not "/", and lies on a mounted local file
#           system, or on a network one while the person has a session
#           (/run/user/UID): no automount is triggered and root has not
#           stat'ed it, so a dead server costs at most one step's time;
#         - what it prints reaches the person running apt.
#       The step may not assume HOME is safe to use: check it with
#       better_os_usable_home, and make every change as the account with
#       better_os_as_account. Pass data to a command as arguments, never
#       inside a program text (sed, awk, sh -c).
#       The whole walk stops after $better_os_total_seconds and says how many
#       people were not reached.
#
#   account-step.sh adds, for steps: better_os_usable_home HOME UID,
#   better_os_owned_real_dir PATH UID, better_os_as_account UID GID CMD...,
#   and better_os_prefix_lines NAME (prefixes each line of its standard
#   input with "Better OS (NAME): ").
#
# Header variables (assignments at the top of the library):
#   better_os_login_defs    login.defs, for the UID_MIN..UID_MAX login range
#   better_os_getent        the program that lists accounts (getent, which
#                           also reaches LDAP, sssd, and systemd-homed
#                           accounts; a test passes a stand-in)
#   better_os_setpriv       the program that runs a command as an account
#   better_os_mounts        the mount table (/proc/self/mounts)
#   better_os_session_root  where logind keeps each session's runtime
#                           directory (/run/user)
#   better_os_step_seconds  how long one person's step may take (30)
#   better_os_total_seconds how long the whole walk may take (120)
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

better_os_home_reachable() {
    # $1 home, $2 uid. Reads the mount table only: the file system the home
    # lies on (the longest mount point that holds it) must be mounted, not an
    # automount point still waiting, and local, unless the person has a
    # session, in which case their home is already mounted and answering.
    # The home is passed through the environment, never into awk's program.
    fstype="$(BETTER_OS_HOME="$1" awk '
        function unescape(s) {
            gsub(/\\040/, " ", s); gsub(/\\011/, "\t", s); gsub(/\\012/, "\n", s); gsub(/\\134/, "\\", s)
            return s
        }
        {
            point = unescape($2)
            home = ENVIRON["BETTER_OS_HOME"]
            prefix = (point == "/") ? "/" : point "/"
            if ((home == point || index(home, prefix) == 1) && length(point) >= best) {
                best = length(point)
                type = $3
            }
        }
        END { print type }' "${better_os_mounts:-/proc/self/mounts}" 2>/dev/null)" || fstype=""
    case "$fstype" in
        '' | autofs) return 1 ;;
        nfs | nfs4 | cifs | smb3 | smbfs | ceph | glusterfs | 9p | sshfs | afs | lustre | davfs | fuse | fuse.*)
            [ -d "${better_os_session_root:-/run/user}/$2" ]
            ;;
        *) return 0 ;;
    esac
}

better_os_for_each_account() {
    # $1: the step, then its own arguments. See the top of this file.
    seconds="${better_os_step_seconds:-30}"
    total="${better_os_total_seconds:-120}"
    uid_min="$(better_os_login_range_value UID_MIN 1000)"
    uid_max="$(better_os_login_range_value UID_MAX 60000)"
    started="$(date +%s)"
    # A directory service that does not answer stops the listing, not the
    # removal.
    accounts="$(timeout -s KILL "$seconds" "${better_os_getent:-getent}" passwd 2>/dev/null)" || true
    printf '%s\n' "$accounts" | {
        late=0
        odd=0
        while IFS=: read -r name _pw uid gid _gecos home _shell; do
            case "$uid" in
                '' | *[!0-9]*) continue ;;
            esac
            # shellcheck disable=SC2015 # both tests pass or the account is skipped
            [ "$uid" -ge "$uid_min" ] && [ "$uid" -le "$uid_max" ] || continue
            case "$gid" in
                '' | *[!0-9]*) continue ;;
            esac
            case "$name" in
                '' | -* | *[!A-Za-z0-9._-]*)
                    odd=$((odd + 1))
                    continue
                    ;;
            esac
            if [ $(($(date +%s) - started)) -ge "$total" ]; then
                late=$((late + 1))
                continue
            fi
            case "$home" in
                /*) ;;
                *) continue ;;
            esac
            better_os_home_reachable "$home" "$uid" || continue
            status=0
            # shellcheck disable=SC2016 # "$@" is the step's own shell's
            timeout -s KILL "$seconds" sh -c "$better_os_account_library"'
"$@"' better-os-step "$@" "$name" "$uid" "$gid" "$home" < /dev/null || status=$?
            case "$status" in
                124 | 137)
                    printf 'Better OS: %s (%s) did not answer within %s seconds; its step was skipped\n' \
                        "$name" "$home" "$seconds" >&2
                    ;;
            esac
        done
        if [ "$odd" -gt 0 ]; then
            printf 'Better OS: %s accounts whose names are not plain names were skipped\n' "$odd" >&2
        fi
        if [ "$late" -gt 0 ]; then
            printf 'Better OS: stopped after %s seconds; %s accounts were not reached\n' "$total" "$late" >&2
        fi
    }
    return 0
}
