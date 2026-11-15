#!/bin/sh
# Updates a privileged service of 0.2.12 or older from outside its own unit
# (ADR 0026, ticket 124). Installed as /usr/libexec/better-os/daemon-handoff by
# better-manager.
#
# Such a service's prerm stops the unit dpkg runs in, so it can never install
# its own successor. When it installs this Better Manager instead, the client
# has already staged the new service and its companions in the service's
# archive. `start` (from this package's postinst) checks for that and starts
# `run` in a transient unit of its own; `run` waits for the old service to go
# idle and installs the staged packages in one apt-get run, outside the old
# unit, where its prerm's stop is harmless.
#
# The state goes to $status (world-readable, two lines: state and detail),
# which Better Manager reads: waiting, installing, done <version>, or
# failed <reason>. Nothing here is silent: every way out writes it.
#
# packaging/tests/daemon-handoff.test.sh sets the paths below to scratch ones.
set -u

expected="${BETTER_OS_HANDOFF_EXPECTED:-/usr/lib/better-os/daemon-handoff/packages}"
archive="${BETTER_OS_HANDOFF_ARCHIVE:-/var/cache/better-os/archives}"
status_dir="${BETTER_OS_HANDOFF_STATUS_DIR:-/run/better-os}"
systemd_dir="${BETTER_OS_HANDOFF_SYSTEMD_DIR:-/run/systemd/system}"
self="${BETTER_OS_HANDOFF_SELF:-/usr/libexec/better-os/daemon-handoff}"
idle_checks="${BETTER_OS_HANDOFF_IDLE_CHECKS:-300}"
idle_interval="${BETTER_OS_HANDOFF_IDLE_INTERVAL:-2}"
status="$status_dir/daemon-handoff"
service=better-manager-daemon
unit=better-manager-daemon-handoff
# The first service whose own maintainer scripts never stop it from inside
# (ticket 105). Better Manager sends anything newer its own update.
first_safe=0.2.13

write_status() {
    mkdir -p "$status_dir" || return 0
    printf '%s\n%s\n' "$1" "${2:-}" > "$status.tmp" &&
        chmod 0644 "$status.tmp" &&
        mv -f "$status.tmp" "$status"
}

# The staged packages whose checksum is the one this release was built with,
# one path per line. A staged file that does not match is not ours.
verified_packages() {
    [ -r "$expected" ] || return 0
    while read -r sum name; do
        [ -n "${name:-}" ] || continue
        file="$archive/$name"
        [ -f "$file" ] || continue
        actual="$(sha256sum "$file" | cut -d' ' -f1)"
        [ "$actual" = "$sum" ] && printf '%s\n' "$file"
    done < "$expected"
}

# True when a verified service package is staged and dpkg holds a service too
# old to take it any other way.
handoff_needed() {
    service_deb="$(verified_packages | grep "/${service}_" | head -n 1)"
    [ -n "$service_deb" ] || return 1
    # shellcheck disable=SC2016 # a dpkg-query format, not a shell expansion
    have="$(dpkg-query -W -f='${Version}' "$service" 2>/dev/null)" || return 1
    [ -n "$have" ] || return 1
    want="$(dpkg-deb -f "$service_deb" Version)" || return 1
    dpkg --compare-versions "$have" lt "$first_safe" &&
        dpkg --compare-versions "$have" lt "$want"
}

old_service_running() {
    if [ -d "$systemd_dir" ]; then
        systemctl is-active --quiet "$service.service" 2>/dev/null
    else
        pidof /usr/libexec/better-manager-daemon >/dev/null 2>&1
    fi
}

start() {
    handoff_needed || return 0
    write_status waiting "the old service is finishing its transaction"
    if [ -d "$systemd_dir" ]; then
        if ! systemd-run --no-block --collect --quiet --unit="$unit" \
            --description='Better OS privileged service handoff' \
            -- "$self" run; then
            write_status failed "systemd-run could not start $unit"
        fi
    else
        # No systemd, so no unit for the old prerm to stop: detached, so it
        # outlives this dpkg run and waits for the lock it holds.
        setsid "$self" run < /dev/null > /dev/null 2>&1 &
    fi
}

run() {
    if ! handoff_needed; then
        write_status failed "no verified $service package is staged for this release"
        return 1
    fi
    checks=0
    while old_service_running; do
        checks=$((checks + 1))
        if [ "$checks" -ge "$idle_checks" ]; then
            write_status failed "the old service did not go idle"
            return 1
        fi
        sleep "$idle_interval"
    done
    packages="$(verified_packages)"
    write_status installing "$(printf '%s' "$packages" | tr '\n' ' ')"
    log="$status_dir/daemon-handoff.log"
    # shellcheck disable=SC2086 # one path per word, none with spaces
    if DEBIAN_FRONTEND=noninteractive apt-get install --yes --no-install-recommends \
        -o DPkg::Lock::Timeout=300 \
        -o Dpkg::Options::=--force-confdef \
        -o Dpkg::Options::=--force-confold \
        -- $packages > "$log" 2>&1; then
        # shellcheck disable=SC2016 # a dpkg-query format, not a shell expansion
        write_status done "$(dpkg-query -W -f='${Version}' "$service" 2>/dev/null)"
    else
        write_status failed "apt-get could not install the new service; see $log"
        return 1
    fi
}

case "${1:-}" in
    start) start ;;
    run) run ;;
    *)
        printf 'usage: %s start|run\n' "$0" >&2
        exit 2
        ;;
esac
