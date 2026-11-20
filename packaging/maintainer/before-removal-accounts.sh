# shellcheck shell=sh disable=SC2154 # build-deb.sh writes the header that sets the better_os_* variables
# Runs a package's before-removal program as each person when apt or dpkg
# removes the package (ticket 90), for a package Better Manager cannot
# remove itself: Better Manager is that package. Better Manager runs a
# component's program in the person's session when it removes the
# component; this is the same program, run by the maintainer scripts, once
# per person, as that person.
#
#   prerm remove           clears any older stamp first thing, makes up a
#                          stamp for this dpkg run, keeps it in
#                          $better_os_run_file, and runs the program, which
#                          undoes what the package changed in the person's
#                          files and sets aside what it replaced, stamped
#   postinst abort-remove  runs it with --undo and the same stamp, because the
#                          removal did not happen; the program uses only a
#                          set-aside file with that stamp
#   postrm remove|purge    deletes what the program set aside, as the person,
#                          and the stamp
#
# Who is a person, the time limit on each person's step, and the account
# switch are accounts.sh's and account-step.sh's, in the same library.
#
# Header variables set by the generated script, besides accounts.sh's:
#   better_os_before_removal_program  the program, by absolute path
#   better_os_before_removal_leftovers what it can leave, relative to the
#                                      home, space separated
#   better_os_run_file                where this dpkg run's stamp is kept
#
# The program runs in a new session (setsid) with standard input from
# /dev/null and its output through a pipe, so it holds nothing of the
# terminal apt runs in, and with a cleared environment holding only HOME, a
# fixed PATH, LC_ALL=C, and the stamp. It finds the person's files under
# their home, and a record kept under a custom XDG_STATE_HOME through the
# location file the window leaves at the default place. Its exit status
# never stops the removal: 3 means not everything could be undone, and what
# it says is printed for the person running apt.

better_os_before_removal_step() {
    # $1 run|undo|forget, $2 stamp, $3 name, $4 uid, $5 gid, $6 home; runs in
    # its own timed shell.
    mode="$1"
    stamp="$2"
    name="$3"
    uid="$4"
    gid="$5"
    home="$6"
    better_os_usable_home "$home" "$uid" || return 0
    case "$mode" in
        run | undo)
            [ -x "$better_os_before_removal_program" ] || return 0
            set --
            [ "$mode" = undo ] && set -- --undo
            better_os_as_account "$uid" "$gid" setsid \
                timeout -s KILL "${better_os_step_seconds:-30}" env -i HOME="$home" \
                PATH=/usr/bin:/bin LC_ALL=C BETTER_OS_REMOVAL_RUN="$stamp" \
                "$better_os_before_removal_program" "$@" < /dev/null 2>&1 \
                | better_os_prefix_lines "$name" || true
            ;;
        forget)
            # A space-separated list of fixed relative paths, split on purpose.
            for leftover in $better_os_before_removal_leftovers; do
                # shellcheck disable=SC2016 # expanded by the inner shell, not this one
                better_os_as_account "$uid" "$gid" sh -c \
                    'p="$1/$2"; if [ -f "$p" ] && [ ! -L "$p" ]; then rm -f -- "$p"; fi' \
                    better-os-forget "$home" "$leftover" < /dev/null || true
            done
            ;;
    esac
    return 0
}

better_os_before_removal_prerm() {
    case "${1:-}" in
        remove) ;;
        *) return 0 ;;
    esac
    # Any older stamp went at the top of this prerm (build-deb.sh puts that
    # line before every other fragment), so an abort-remove can only read
    # this run's.
    stamp="$(date +%s%N).$$"
    mkdir -p "$(dirname "$better_os_run_file")" 2>/dev/null || true
    printf '%s\n' "$stamp" > "$better_os_run_file" 2>/dev/null || true
    better_os_for_each_account better_os_before_removal_step run "$stamp"
    return 0
}

better_os_before_removal_postinst() {
    case "${1:-}" in
        abort-remove) ;;
        *) return 0 ;;
    esac
    stamp=""
    if [ -f "$better_os_run_file" ]; then
        stamp="$(head -n 1 "$better_os_run_file" 2>/dev/null)" || stamp=""
    fi
    # Without this run's stamp the program deletes what is set aside unused.
    better_os_for_each_account better_os_before_removal_step undo "$stamp"
    rm -f "$better_os_run_file" 2>/dev/null || true
    return 0
}

better_os_before_removal_postrm() {
    case "${1:-}" in
        remove | purge) ;;
        *) return 0 ;;
    esac
    better_os_for_each_account better_os_before_removal_step forget ""
    rm -f "$better_os_run_file" 2>/dev/null || true
    return 0
}
