#!/usr/bin/env bash
# Run as root inside a disposable VM. Starts a program in USER's running
# graphical session, with the environment the session imported into the user
# manager, as a transient user unit whose output goes to the user journal.
#
#   in-session.sh USER UNIT PROGRAM [ARGS...]
#   journalctl --user -u UNIT    (as USER) shows what it printed
set -euo pipefail
U="$1"; UNIT="$2"; shift 2
UID_="$(id -u "$U")"
runuser -u "$U" -- env XDG_RUNTIME_DIR="/run/user/$UID_" \
    DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$UID_/bus" \
    systemd-run --user --quiet --collect --unit="$UNIT" -- "$@"
