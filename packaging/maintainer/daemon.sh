# shellcheck shell=sh disable=SC2154 # build-deb.sh writes the header that sets the better_os_* variables
# Better OS maintainer-script function for better-manager-daemon (ticket 105).
# `packaging/build-deb.sh` copies this file into the daemon package's postinst
# after a header that sets:
#
#   better_os_daemon_busy  /run/better-os/daemon-busy
#
# `packaging/tests/maintainer-scripts.test.sh` sets it to a scratch path.

# True while the daemon is in the middle of a transaction or a repair. The
# daemon creates the marker when one starts and removes it when it ends; it
# removes a marker left by a daemon that died when it starts, and /run is
# emptied at boot, so a marker never holds a restart back for good.
better_os_daemon_transaction_in_progress() {
    [ -e "$better_os_daemon_busy" ]
}
