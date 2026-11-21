#!/usr/bin/env bash
# The nested cosmic-comp for tests (ticket 200; engineering/specs/better-os-session.md,
# "The nested test compositor", says what it can and cannot show).
#
# Nothing is installed on the host. `fetch` downloads System76's released
# cosmic-comp over HTTPS, checks InRelease against the one fingerprint ADR
# 0038 names, Packages.gz and the .deb against InRelease's SHA-256 sums, and
# unpacks the .deb with dpkg-deb -x into a scratch directory. `start` runs it
# nested in a headless sway, both under AGENTS.md's sandbox rules: one
# transient scope with MemoryMax=4G and MemorySwapMax=0, a no-op ibus-daemon
# first on PATH, a private D-Bus session (dbus-run-session), HOME and every
# XDG directory temporary, and ended by PID with `stop`.
#
#   packaging/tests/cosmic-nested.sh fetch
#   packaging/tests/cosmic-nested.sh start [--scale S] [--size WxH]
#       cosmic-comp with one output of WxH logical pixels at scale S
#       (default 1280x800 at 1). Its winit backend has exactly one output.
#   packaging/tests/cosmic-nested.sh start --sway-only [--outputs N] [--scale S] [--size WxH]
#       no cosmic-comp: clients talk to the headless sway itself, which can
#       add and remove outputs (`output add|remove`). For what needs more
#       than one output.
#   packaging/tests/cosmic-nested.sh exec -- PROGRAM [ARGS...]   a client of the nested compositor
#   packaging/tests/cosmic-nested.sh swaymsg ARGS...             the outer sway's IPC
#   packaging/tests/cosmic-nested.sh shot FILE.png [OUTPUT]      a screenshot of a sway output
#   packaging/tests/cosmic-nested.sh click X Y                   a left click, in the client's logical pixels
#   packaging/tests/cosmic-nested.sh keys TEXT                   typed on a virtual keyboard ({Return}, {Escape}, ...)
#   packaging/tests/cosmic-nested.sh output add|remove [NAME]    --sway-only: hotplug a headless output
#   packaging/tests/cosmic-nested.sh status | bus-check | stop
#
# `shot`, `click` and `keys` use better-wl-probe from crates/layer-check:
# set BETTER_WL_PROBE to it, or it is looked for in target/debug.
# State lives in $BETTER_OS_COSMIC_NESTED (default ~/.cache/better-os/cosmic-nested).
set -euo pipefail

ROOT="${BETTER_OS_COSMIC_NESTED:-$HOME/.cache/better-os/cosmic-nested}"
REPO=https://apt.pop-os.org/release
SUITE=noble
ARCH=amd64
# ADR 0038 and ticket 204: System76's archive key, the one key trusted here.
FINGERPRINT=63C46DF0140D738961429F4E204DD8AEC33A7AFF
KEY_URL="https://keyserver.ubuntu.com/pks/lookup?op=get&search=0x$FINGERPRINT"
RUN="$ROOT/run"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROBE="${BETTER_WL_PROBE:-$HERE/../../target/debug/better-wl-probe}"

die() { printf 'cosmic-nested: %s\n' "$*" >&2; exit 1; }
say() { printf 'cosmic-nested: %s\n' "$*" >&2; }

# --- fetch ---------------------------------------------------------------

fetch() {
    local work="$ROOT/download" keyring="$ROOT/download/pop.gpg"
    mkdir -p "$work" "$ROOT/gnupg"
    chmod 700 "$ROOT/gnupg"

    curl -fsS --proto '=https' -o "$work/key.asc" "$KEY_URL"
    rm -f "$keyring"
    GNUPGHOME="$ROOT/gnupg" gpg --batch --quiet --no-default-keyring \
        --keyring "$keyring" --import "$work/key.asc"
    # Exactly the named primary key, nothing else from the key server.
    local primaries
    primaries="$(GNUPGHOME="$ROOT/gnupg" gpg --batch --no-default-keyring \
        --keyring "$keyring" --with-colons --list-keys | awk -F: '/^pub/{getline; print $10}')"
    [[ "$primaries" == "$FINGERPRINT" ]] || die "the key server returned $primaries, not $FINGERPRINT"

    curl -fsS --proto '=https' -o "$work/InRelease" "$REPO/dists/$SUITE/InRelease"
    LC_ALL=C gpgv --keyring "$keyring" --output "$work/Release" "$work/InRelease" 2>"$work/gpgv.log" \
        || { cat "$work/gpgv.log" >&2; die "InRelease is not signed by $FINGERPRINT"; }
    grep -q "$FINGERPRINT" "$work/gpgv.log" || die "InRelease was signed by another key"

    local packages="main/binary-$ARCH/Packages.gz" want got
    curl -fsS --proto '=https' -o "$work/Packages.gz" "$REPO/dists/$SUITE/$packages"
    want="$(awk -v f="$packages" '/^SHA256:/{s=1;next} /^[^ ]/{s=0} s && $3==f {print $1}' "$work/Release")"
    got="$(sha256sum "$work/Packages.gz" | cut -d' ' -f1)"
    [[ -n "$want" && "$want" == "$got" ]] || die "Packages.gz: InRelease says $want, the file is $got"
    gzip -dc "$work/Packages.gz" > "$work/Packages"

    # The newest cosmic-comp listed, by dpkg's own ordering.
    local version stanza filename sha
    version="$(awk -v RS= '$0 ~ /(^|\n)Package: cosmic-comp\n/' "$work/Packages" \
        | awk '/^Version:/{print $2}' \
        | while read -r v; do printf '%s\n' "$v"; done \
        | sort -u | { best=; while read -r v; do
            if [[ -z "$best" ]] || dpkg --compare-versions "$v" gt "$best"; then best="$v"; fi
          done; printf '%s\n' "$best"; })"
    [[ -n "$version" ]] || die "no cosmic-comp in $SUITE/$ARCH"
    stanza="$(awk -v RS= -v v="$version" '$0 ~ /(^|\n)Package: cosmic-comp\n/ && index($0, "\nVersion: " v "\n")' "$work/Packages")"
    filename="$(awk '/^Filename:/{print $2}' <<< "$stanza")"
    sha="$(awk '/^SHA256:/{print $2}' <<< "$stanza")"
    local deb
    deb="$work/$(basename "$filename")"
    if [[ ! -f "$deb" ]] || [[ "$(sha256sum "$deb" | cut -d' ' -f1)" != "$sha" ]]; then
        curl -fsS --proto '=https' -o "$deb" "$REPO/$filename"
    fi
    got="$(sha256sum "$deb" | cut -d' ' -f1)"
    [[ "$got" == "$sha" ]] || die "$(basename "$deb"): Packages says $sha, the file is $got"

    rm -rf "$ROOT/root"
    mkdir -p "$ROOT/root"
    dpkg-deb -x "$deb" "$ROOT/root"
    printf '%s\n' "$version" > "$ROOT/root/VERSION"
    say "cosmic-comp $version unpacked into $ROOT/root (sha256 $sha)"

    local missing
    missing="$(ldd "$ROOT/root/usr/bin/cosmic-comp" | grep 'not found' || true)"
    [[ -z "$missing" ]] || die "libraries the host does not have:"$'\n'"$missing"
    say "every library cosmic-comp links resolves against the host's"
}

# --- start and stop ------------------------------------------------------

pidfile() { printf '%s/scope.pid' "$ROOT"; }

running() {
    [[ -f "$(pidfile)" ]] && kill -0 "$(cat "$(pidfile)")" 2>/dev/null
}

start() {
    local outputs=1 scale=1 size=1280x800 sway_only=0
    while (($#)); do
        case "$1" in
            --outputs) outputs="$2"; shift 2 ;;
            --scale) scale="$2"; shift 2 ;;
            --size) size="$2"; shift 2 ;;
            --sway-only) sway_only=1; shift ;;
            *) die "start: unknown argument $1" ;;
        esac
    done
    ((sway_only)) || [[ -x "$ROOT/root/usr/bin/cosmic-comp" ]] || die "run '$0 fetch' first"
    ((sway_only)) || ((outputs == 1)) || die "cosmic-comp's nested backends have one output; use --sway-only for more"
    running && die "already running (scope pid $(cat "$(pidfile)")); '$0 stop' first"

    rm -rf "$RUN"
    mkdir -p "$RUN"/{xdg,home,config,data,state/cosmic-comp,cache,bin,log}
    chmod 700 "$RUN/xdg"
    # A sandbox desktop or GTK program would start its own ibus-daemon; this
    # one does nothing (AGENTS.md).
    printf '#!/bin/sh\nexec sleep infinity\n' > "$RUN/bin/ibus-daemon"
    chmod +x "$RUN/bin/ibus-daemon"

    local width="${size%x*}" height="${size#*x}" pw ph sway_scale i
    if ((sway_only)); then
        # sway itself scales: each output is WxH logical at S.
        sway_scale="$scale"
        pw="$(awk -v a="$width" -v s="$scale" 'BEGIN{printf "%d", a*s}')"
        ph="$(awk -v a="$height" -v s="$scale" 'BEGIN{printf "%d", a*s}')"
    else
        # sway shows cosmic-comp's window 1:1 in physical pixels; cosmic-comp
        # scales its own output, from the outputs.ron it reads at start. The
        # mode there must be the winit window's size when cosmic-comp reads
        # it, winit's default 1280x800, before sway makes it fullscreen;
        # any other size is refused ("Cannot set window size",
        # src/backend/winit.rs apply_config_for_outputs) and the whole entry,
        # scale included, is reset. The scale survives the resize after.
        sway_scale=1
        pw="$(awk -v a="$width" -v s="$scale" 'BEGIN{printf "%d", a*s}')"
        ph="$(awk -v a="$height" -v s="$scale" 'BEGIN{printf "%d", a*s}')"
        cat > "$RUN/state/cosmic-comp/outputs.ron" <<RON
(
    config: {
        [
            (
                connector: "WINIT-0",
                make: "COSMIC",
                model: "WINIT-0",
            ),
        ]: [
            (
                mode: ((1280, 800), None),
                vrr: r#true,
                scale: $scale,
                transform: Flipped180,
                position: (0, 0),
                enabled: r#true,
                xwayland_primary: true,
            ),
        ],
    },
)
RON
    fi
    {
        printf 'xwayland disable\ndefault_border none\nfocus_follows_mouse no\n'
        # Side by side in sway's logical layout.
        for ((i = 1; i <= outputs; i++)); do
            printf 'output HEADLESS-%d resolution %dx%d scale %s position %d 0\n' \
                "$i" "$pw" "$ph" "$sway_scale" "$(((i - 1) * width))"
        done
        if ((sway_only)); then
            # Clients are sway's own: a maximized-looking window tiles; a
            # fullscreen one would cover the top layer.
            :
        else
            # cosmic-comp's winit window fills sway's output.
            printf 'for_window [title="COSMIC"] fullscreen enable\n'
            printf 'for_window [app_id=".*"] fullscreen enable\n'
        fi
    } > "$RUN/sway.config"
    printf '%s %s %s %s %s\n' "$sway_only" "$scale" "$pw" "$ph" "$outputs" > "$RUN/mode"

    cat > "$RUN/inner.sh" <<'INNER'
#!/usr/bin/env bash
set -u
RUN="$1"; ROOT="$2"; outputs="$3"; sway_only="$4"
export PATH="$RUN/bin:$PATH"
export XDG_RUNTIME_DIR="$RUN/xdg" HOME="$RUN/home" XDG_CONFIG_HOME="$RUN/config"
export XDG_DATA_HOME="$RUN/data" XDG_STATE_HOME="$RUN/state" XDG_CACHE_HOME="$RUN/cache"
export XDG_CONFIG_DIRS="$RUN/config-dirs" XDG_DATA_DIRS="$ROOT/root/usr/share:/usr/share"
unset WAYLAND_DISPLAY DISPLAY WAYLAND_SOCKET SWAYSOCK XDG_SESSION_ID XDG_SEAT XDG_VTNR
export XDG_SESSION_TYPE=wayland
export WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS="$outputs" WLR_LIBINPUT_NO_DEVICES=1
export WLR_RENDERER=gles2
sway -c "$RUN/sway.config" > "$RUN/log/sway.log" 2>&1 &
sway_pid=$!
sock=
for _ in $(seq 100); do
    sock="$(ls "$XDG_RUNTIME_DIR" | grep -E '^wayland-[0-9]+$' | head -1)"
    [[ -n "$sock" ]] && break
    sleep 0.1
done
[[ -n "$sock" ]] || { echo "sway did not start" >&2; kill "$sway_pid"; exit 1; }
sleep 0.3
{
    printf 'SWAY_WAYLAND_DISPLAY=%s\n' "$sock"
    printf 'SWAYSOCK=%s\n' "$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock 2>/dev/null | head -1)"
    printf 'SWAY_PID=%s\n' "$sway_pid"
} > "$RUN/sway.env"
if [[ "$sway_only" == 1 ]]; then
    {
        printf 'WAYLAND_DISPLAY=%s\nCOSMIC_PID=\n' "$sock"
        printf 'DBUS_SESSION_BUS_ADDRESS=%s\n' "$DBUS_SESSION_BUS_ADDRESS"
    } > "$RUN/nested.env"
    wait "$sway_pid"
    exit 0
fi
before="$(ls "$XDG_RUNTIME_DIR" | grep -E '^wayland-[0-9]+$' | sort)"
WAYLAND_DISPLAY="$sock" COSMIC_BACKEND=winit XDG_CURRENT_DESKTOP=COSMIC COSMIC_DISABLE_XWAYLAND=1 \
    RUST_LOG="${RUST_LOG:-warn}" \
    "$ROOT/root/usr/bin/cosmic-comp" > "$RUN/log/cosmic-comp.log" 2>&1 &
comp_pid=$!
nested=
for _ in $(seq 150); do
    nested="$(comm -13 <(printf '%s\n' "$before") <(ls "$XDG_RUNTIME_DIR" | grep -E '^wayland-[0-9]+$' | sort) | head -1)"
    [[ -n "$nested" ]] && break
    kill -0 "$comp_pid" 2>/dev/null || break
    sleep 0.1
done
[[ -n "$nested" ]] || { echo "cosmic-comp did not start" >&2; kill "$comp_pid" "$sway_pid"; exit 1; }
sleep 0.5
{
    printf 'WAYLAND_DISPLAY=%s\nCOSMIC_PID=%s\n' "$nested" "$comp_pid"
    printf 'DBUS_SESSION_BUS_ADDRESS=%s\n' "$DBUS_SESSION_BUS_ADDRESS"
} > "$RUN/nested.env"
wait "$comp_pid"
kill "$sway_pid" 2>/dev/null
wait
INNER
    chmod +x "$RUN/inner.sh"

    # The private bus may activate only what is in its own empty service
    # directory, never the services installed on this machine (Better OS's
    # own among them), and it is started with the sandbox's HOME and XDG
    # directories so nothing it starts reads the person's settings.
    mkdir -p "$RUN/dbus-services"
    cat > "$RUN/bus.conf" <<CONF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <keep_umask/>
  <listen>unix:dir=$RUN/xdg</listen>
  <servicedir>$RUN/dbus-services</servicedir>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
CONF

    setsid systemd-run --user --scope --quiet --collect \
        --unit="better-os-cosmic-nested-$$" \
        -p MemoryMax=4G -p MemorySwapMax=0 \
        -- env HOME="$RUN/home" XDG_RUNTIME_DIR="$RUN/xdg" XDG_CONFIG_HOME="$RUN/config" \
        XDG_DATA_HOME="$RUN/data" XDG_STATE_HOME="$RUN/state" XDG_CACHE_HOME="$RUN/cache" \
        PATH="$RUN/bin:$PATH" \
        dbus-run-session --config-file="$RUN/bus.conf" -- \
        bash "$RUN/inner.sh" "$RUN" "$ROOT" "$outputs" "$sway_only" \
        > "$RUN/log/scope.log" 2>&1 &
    printf '%s\n' "$!" > "$(pidfile)"

    for _ in $(seq 200); do
        [[ -f "$RUN/nested.env" ]] && break
        running || { tail -n 20 "$RUN/log/"*.log >&2; die "the scope ended before the compositor was up"; }
        sleep 0.1
    done
    [[ -f "$RUN/nested.env" ]] || die "the compositor was not up after 20 s; logs in $RUN/log"
    bus_check || { stop; die "the sandbox bus has a name it should not: see above"; }
    if ((sway_only)); then
        say "up: headless sway with $outputs output(s) at scale $scale, scope pid $(cat "$(pidfile)")"
    else
        say "up: cosmic-comp $(cat "$ROOT/root/VERSION"), one output ${width}x${height} at scale $scale, scope pid $(cat "$(pidfile)")"
    fi
}

# No installed service may appear on the sandbox's bus: every name on it
# must belong to a process this sandbox started.
bus_check() {
    local address name pid bad=0
    address="$(grep '^DBUS_SESSION_BUS_ADDRESS=' "$RUN/nested.env" | cut -d= -f2-)"
    while read -r name pid _; do
        [[ "$name" == :* || "$name" == org.freedesktop.DBus ]] && continue
        # A name is fine only when its owner runs inside this sandbox's scope.
        if [[ "$name" == *[Bb]etter* ]] || ! grep -q 'better-os-cosmic-nested-' "/proc/$pid/cgroup" 2>/dev/null; then
            printf 'cosmic-nested: %s is owned by pid %s, outside the sandbox\n' "$name" "$pid" >&2
            bad=1
        fi
    done < <(DBUS_SESSION_BUS_ADDRESS="$address" busctl --user list --no-legend 2>/dev/null)
    return "$bad"
}

# Ends every process connected to this sandbox's bus, found by the bus
# address in its environment: all of them were started inside the scope.
end_bus_clients() {
    local address pid
    [[ -f "$RUN/nested.env" ]] || return 0
    address="$(grep '^DBUS_SESSION_BUS_ADDRESS=' "$RUN/nested.env" | cut -d= -f2-)"
    [[ -n "$address" ]] || return 0
    for pid in $(pgrep -u "$(id -u)" .); do
        [[ "$pid" == "$$" ]] && continue
        # Processes of other users' sessions or already gone are skipped.
        if { tr '\0' '\n' < "/proc/$pid/environ"; } 2>/dev/null | grep -qxF "DBUS_SESSION_BUS_ADDRESS=$address"; then
            kill "$pid" 2>/dev/null || true
        fi
    done
}

stop() {
    if ! running; then
        say "not running"
        rm -f "$(pidfile)"
        return 0
    fi
    local pid
    pid="$(cat "$(pidfile)")"
    # Only processes this script started: cosmic-comp and sway by their
    # recorded PIDs, then the scope's own process group.
    # shellcheck disable=SC1091
    { . "$RUN/nested.env"; . "$RUN/sway.env"; } 2>/dev/null || true
    [[ -n "${COSMIC_PID:-}" ]] && kill "$COSMIC_PID" 2>/dev/null || true
    [[ -n "${SWAY_PID:-}" ]] && kill "$SWAY_PID" 2>/dev/null || true
    for _ in $(seq 50); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    end_bus_clients
    if kill -0 "$pid" 2>/dev/null; then kill -- "-$pid" 2>/dev/null || kill "$pid" 2>/dev/null || true; fi
    rm -f "$(pidfile)"
    say "stopped"
}

status() {
    running || { say "not running"; return 1; }
    cat "$RUN/nested.env" "$RUN/sway.env"
}

# --- clients -------------------------------------------------------------

client_env() {
    running && [[ -f "$RUN/nested.env" ]] || die "not running"
    set -a
    # shellcheck disable=SC1091
    . "$RUN/nested.env"; . "$RUN/sway.env"
    set +a
    export PATH="$RUN/bin:$PATH" XDG_RUNTIME_DIR="$RUN/xdg" HOME="$RUN/home"
    export XDG_CONFIG_HOME="$RUN/config" XDG_DATA_HOME="$RUN/data"
    export XDG_STATE_HOME="$RUN/state" XDG_CACHE_HOME="$RUN/cache"
    export XDG_CURRENT_DESKTOP=BetterOS:COSMIC XDG_SESSION_TYPE=wayland
    unset DISPLAY WAYLAND_SOCKET
}

probe() {
    [[ -x "$PROBE" ]] || die "no better-wl-probe at $PROBE; build crates/layer-check or set BETTER_WL_PROBE"
    "$PROBE" "$@"
}

cmd="${1:-}"; shift || true
case "$cmd" in
    fetch) fetch "$@" ;;
    start) start "$@" ;;
    stop) stop ;;
    status) status ;;
    bus-check)
        running || die "not running"
        bus_check && say "the sandbox bus has only the sandbox's own names"
        ;;
    exec)
        [[ "${1:-}" == "--" ]] && shift
        (($#)) || die "exec: nothing to run"
        client_env
        exec "$@"
        ;;
    swaymsg)
        client_env
        exec swaymsg -s "$SWAYSOCK" "$@"
        ;;
    shot)
        (($# >= 1)) || die "shot FILE.png [OUTPUT]"
        client_env
        WAYLAND_DISPLAY="$SWAY_WAYLAND_DISPLAY" probe shot "$1" --output "${2:-HEADLESS-1}"
        ;;
    click)
        (($# == 2 || $# == 3)) || die "click X Y [OUTPUT]"
        client_env
        read -r sway_only scale pw ph _ < "$RUN/mode"
        # The headless seat has no pointer of its own (capabilities 0), so
        # the probe adds a virtual one to sway. In cosmic mode sway's pixels
        # are cosmic-comp's physical ones; in sway-only mode sway's logical
        # pixels are the client's.
        if ((sway_only)); then
            x="$1"; y="$2"
            w="$(awk -v a="$pw" -v s="$scale" 'BEGIN{printf "%d", a/s}')"
            h="$(awk -v a="$ph" -v s="$scale" 'BEGIN{printf "%d", a/s}')"
        else
            x="$(awk -v a="$1" -v s="$scale" 'BEGIN{printf "%d", a*s}')"
            y="$(awk -v a="$2" -v s="$scale" 'BEGIN{printf "%d", a*s}')"
            w="$pw"; h="$ph"
        fi
        WAYLAND_DISPLAY="$SWAY_WAYLAND_DISPLAY" probe click "$x" "$y" "$w" "$h" --output "${3:-HEADLESS-1}"
        ;;
    keys)
        (($# == 1)) || die "keys TEXT"
        client_env
        probe keys "$1"
        ;;
    output)
        client_env
        read -r sway_only _ < "$RUN/mode"
        ((sway_only)) || die "output: only with start --sway-only"
        case "${1:-}" in
            add) swaymsg -s "$SWAYSOCK" -q create_output ;;
            remove) swaymsg -s "$SWAYSOCK" -q "output ${2:?output remove NAME} unplug" ;;
            *) die "output add | output remove NAME" ;;
        esac
        ;;
    *) sed -n '2,32p' "$0"; exit 2 ;;
esac
