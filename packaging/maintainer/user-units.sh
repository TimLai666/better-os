# Better OS maintainer-script functions for a package's systemd user units
# (ticket 105). `packaging/build-deb.sh` copies this file into the preinst and
# postinst of every package that ships a unit under /usr/lib/systemd/user,
# after a header that sets:
#
#   better_os_package       the package's name
#   better_os_user_units    the user units it ships, separated by spaces
#   better_os_unit_dir      /usr/lib/systemd/user
#   better_os_run_user_root /run/user
#   better_os_state_dir     /run/better-os/upgrade
#   better_os_path          the PATH systemd-run and systemctl are found on
#   better_os_cgroup_file   /proc/self/cgroup
#
# `packaging/tests/maintainer-scripts.test.sh` sets them to scratch paths and
# puts a fake systemctl first on PATH. Nothing here ever starts a unit that a
# person turned off: a running unit is restarted with try-restart, which does
# nothing to a stopped one, and a unit that is new in this version is enabled
# only for a person who has the component's other units enabled. Better
# Manager restarts the same units after it updates a component; the two never
# disagree, because try-restart only ever restarts what is already running.
#
# This is POSIX sh: dpkg runs maintainer scripts with /bin/sh.

# Prints the uid of every person whose systemd user manager is running.
better_os_logged_in_uids() {
    for better_os_dir in "$better_os_run_user_root"/*; do
        better_os_uid="${better_os_dir##*/}"
        case "$better_os_uid" in
            '' | *[!0-9]*) continue ;;
        esac
        [ "$better_os_uid" -ne 0 ] || continue
        [ -S "$better_os_dir/systemd/private" ] || continue
        printf '%s\n' "$better_os_uid"
    done
}

# Runs `systemctl --user` against this person's own manager. systemd starts
# it as a short-lived service of its own, as that user, with nothing inherited
# from this script — no descriptor, no variable — and no login session: no PAM
# stack runs, so nothing is written to the login records and no message of the
# day is generated. It talks to the manager over the private socket in the
# person's runtime directory, and is stopped after 30 seconds if the manager
# does not answer.
better_os_user_systemctl() {
    better_os_who="$1"
    shift
    env -i PATH="$better_os_path" \
        systemd-run --wait --quiet --collect \
        -p User="$better_os_who" -p RuntimeMaxSec=30 \
        -E XDG_RUNTIME_DIR="$better_os_run_user_root/$better_os_who" \
        systemctl --user "$@"
}

# preinst: remembers which of the package's user units the installed version
# already has, so the postinst can tell a unit that is new in this one.
better_os_user_units_before_upgrade() {
    [ "$1" = upgrade ] || return 0
    mkdir -p "$better_os_state_dir"
    better_os_record="$better_os_state_dir/$better_os_package.units"
    : > "$better_os_record"
    for better_os_unit in $better_os_user_units; do
        if [ -e "$better_os_unit_dir/$better_os_unit" ]; then
            printf '%s\n' "$better_os_unit" >> "$better_os_record"
        fi
    done
}

# postinst configure: for every person logged in, reloads their manager,
# enables and starts the units that are new in this version if they had the
# component enabled, and restarts the units that are running. $1 is the
# version that was configured before, empty on a first install, where Better
# Manager's own install step enables the units.
better_os_user_units_after_upgrade() {
    better_os_record="$better_os_state_dir/$better_os_package.units"
    if [ -z "$1" ]; then
        rm -f "$better_os_record"
        return 0
    fi
    # Without a record every unit counts as old: nothing new is enabled.
    if [ -f "$better_os_record" ]; then
        better_os_old_units=""
        better_os_new_units=""
        for better_os_unit in $better_os_user_units; do
            if grep -qxF "$better_os_unit" "$better_os_record"; then
                better_os_old_units="$better_os_old_units $better_os_unit"
            else
                better_os_new_units="$better_os_new_units $better_os_unit"
            fi
        done
    else
        better_os_old_units="$better_os_user_units"
        better_os_new_units=""
    fi
    for better_os_uid in $(better_os_logged_in_uids); do
        better_os_user_systemctl "$better_os_uid" daemon-reload || continue
        better_os_component_on=no
        for better_os_unit in $better_os_old_units; do
            if better_os_user_systemctl "$better_os_uid" is-enabled --quiet "$better_os_unit"; then
                better_os_component_on=yes
            fi
        done
        if [ "$better_os_component_on" = yes ]; then
            for better_os_unit in $better_os_new_units; do
                better_os_user_systemctl "$better_os_uid" enable --now --no-block \
                    "$better_os_unit" || true
            done
        fi
        for better_os_unit in $better_os_old_units; do
            better_os_user_systemctl "$better_os_uid" try-restart --no-block \
                "$better_os_unit" || true
        done
    done
    rm -f "$better_os_record"
}

# True when this script runs inside better-manager-daemon.service: Better
# Manager's privileged service is installing the package. That unit runs with
# ProtectHome=yes, which hides /run/user, so no person's manager can be reached
# from in there.
better_os_inside_daemon_unit() {
    grep -q '/better-manager-daemon\.service$' "$better_os_cgroup_file" 2>/dev/null
}

# The postinst's entry point. "$0" is the postinst itself, so from inside the
# daemon's sandbox it runs again as a short-lived system service, outside it,
# with the mode "better-os-user-units", and waits for it.
better_os_user_units_postinst() {
    case "$1" in
        configure)
            if better_os_inside_daemon_unit && command -v systemd-run >/dev/null 2>&1; then
                systemd-run --quiet --wait --collect --service-type=exec \
                    "$0" better-os-user-units "${2:-}" && return 0
            fi
            better_os_user_units_after_upgrade "${2:-}"
            ;;
        better-os-user-units)
            better_os_user_units_after_upgrade "${2:-}"
            ;;
    esac
}
