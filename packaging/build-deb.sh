#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="$ROOT_DIR/dist"
VERSION="${VERSION:-}"
ARCH="${DEB_HOST_ARCH:-}"
RELEASE_TARGET="local"
MAINTAINER="TimLai666 <tim930102@icloud.com>"

usage() {
    printf 'Usage: %s [--output-dir DIR] [--target TARGET]\n' "$0"
}

while (($# > 0)); do
    case "$1" in
        --output-dir)
            if (($# < 2)); then
                usage >&2
                exit 2
            fi
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --target)
            if (($# < 2)); then
                usage >&2
                exit 2
            fi
            RELEASE_TARGET="$2"
            shift 2
            ;;
        --help)
            usage
            exit 0
            ;;
        *)
            usage >&2
            exit 2
            ;;
    esac
done

for command_name in cargo dpkg dpkg-deb dpkg-shlibdeps install sha256sum jq; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        printf 'Missing required command: %s\n' "$command_name" >&2
        exit 1
    fi
done

if [[ -z "$VERSION" ]]; then
    VERSION="$(awk '
        $0 == "[workspace.package]" { in_section = 1; next }
        /^\[/ { in_section = 0 }
        in_section && $1 == "version" {
            gsub(/"/, "", $3)
            print $3
            exit
        }
    ' "$ROOT_DIR/Cargo.toml")"
fi

if [[ -z "$VERSION" || ! "$VERSION" =~ ^[0-9][0-9A-Za-z.+:~-]*$ ]]; then
    printf 'Invalid package version: %s\n' "$VERSION" >&2
    exit 1
fi

HOST_ARCH="$(dpkg --print-architecture)"
if [[ -z "$ARCH" ]]; then
    ARCH="$HOST_ARCH"
fi

if [[ "$ARCH" != "$HOST_ARCH" ]]; then
    printf 'Cross-architecture packaging is not supported by this step: host=%s requested=%s\n' \
        "$HOST_ARCH" "$ARCH" >&2
    exit 1
fi

if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
    printf 'CARGO_BUILD_TARGET is not supported by this host-only packaging step: %s\n' \
        "$CARGO_BUILD_TARGET" >&2
    exit 1
fi

case "$ARCH" in
    amd64|arm64)
        ;;
    *)
        printf 'Unsupported Debian architecture for this packaging step: %s\n' "$ARCH" >&2
        exit 1
        ;;
esac

if [[ ! "$RELEASE_TARGET" =~ ^[a-z0-9][a-z0-9.-]*$ ]]; then
    printf 'Invalid release target: %s\n' "$RELEASE_TARGET" >&2
    exit 1
fi

cd "$ROOT_DIR"
export RUST_FONTCONFIG_DLOPEN="${RUST_FONTCONFIG_DLOPEN:-1}"
"$ROOT_DIR/packaging/generate-third-party-notices.sh" --check
cargo build --release \
    -p manager-gui \
    -p manager-daemon \
    -p monitor-gui \
    -p monitor-service \
    -p monitor-cli \
    -p launcher-gui \
    -p files-gui \
    -p touchpad-gui \
    -p awake-service \
    -p awake-tray \
    -p awake-gui \
    -p storage-service \
    -p storage-platform

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
BUILD_DIR="$TARGET_DIR/release"
mkdir -p "$OUTPUT_DIR"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

# Every desktop package is the same shape — binaries, payload data, license
# notices, derived dependencies, checksum sidecar — and differs only in what
# goes into it. The caller fills these four arrays and calls make_package; each
# call resets them, so nothing leaks from one package into the next.
#
#   PACKAGE_BINARIES  built binary name -> installed path under the package root
#   PACKAGE_DATA      repository-relative source file -> installed path
#   PACKAGE_DEPENDS   extra runtime dependencies, added to the derived ones
#   PACKAGE_RECOMMENDS  a single Recommends field value, or empty
PACKAGE_BINARIES=()
PACKAGE_DATA=()
PACKAGE_DEPENDS=()
PACKAGE_RECOMMENDS=""

reset_package_inputs() {
    PACKAGE_BINARIES=()
    PACKAGE_DATA=()
    PACKAGE_DEPENDS=()
    PACKAGE_RECOMMENDS=""
}

# The GPUI toolkit dlopens fontconfig and the Wayland client libraries, so they
# are not in the derived shared-library list and have to be named. A package
# with no window ships none of them; see make_daemon_package and better-storage.
GRAPHICS_DEPENDS='libfontconfig1, libwayland-client0, libwayland-egl1, libwayland-cursor0'

make_package() {
    local package_name="$1"
    local description="$2"
    local description_body="$3"
    local staging_dir="$WORK_DIR/$package_name"
    local dependency_control_dir="$WORK_DIR/$package_name-deps"
    local deb_filename="${package_name}_${VERSION}_${RELEASE_TARGET}_${ARCH}.deb"
    local deb_path="$OUTPUT_DIR/$deb_filename"
    local shlib_dependencies
    local runtime_dependencies
    local entry binary_name installed_path source_path
    local scanned_binaries=()

    if ((${#PACKAGE_BINARIES[@]} == 0)); then
        printf 'No binaries declared for %s\n' "$package_name" >&2
        exit 1
    fi

    mkdir -p "$staging_dir/DEBIAN" "$staging_dir/usr/share/doc/$package_name"

    for entry in "${PACKAGE_BINARIES[@]}"; do
        binary_name="${entry%%:*}"
        installed_path="${entry#*:}"
        if [[ ! -x "$BUILD_DIR/$binary_name" ]]; then
            printf 'Missing release binary: %s\n' "$BUILD_DIR/$binary_name" >&2
            exit 1
        fi
        mkdir -p "$staging_dir/$(dirname "$installed_path")"
        install -m 0755 "$BUILD_DIR/$binary_name" "$staging_dir/$installed_path"
        scanned_binaries+=("$staging_dir/$installed_path")
    done

    for entry in ${PACKAGE_DATA[@]+"${PACKAGE_DATA[@]}"}; do
        source_path="${entry%%:*}"
        installed_path="${entry#*:}"
        if [[ ! -f "$ROOT_DIR/$source_path" ]]; then
            printf 'Missing packaging data file: %s\n' "$ROOT_DIR/$source_path" >&2
            exit 1
        fi
        mkdir -p "$staging_dir/$(dirname "$installed_path")"
        install -m 0644 "$ROOT_DIR/$source_path" "$staging_dir/$installed_path"
    done

    install -m 0644 "$ROOT_DIR/LICENSE" "$staging_dir/usr/share/doc/$package_name/copyright"
    install -m 0644 "$ROOT_DIR/docs/third-party-licenses.md" \
        "$staging_dir/usr/share/doc/$package_name/THIRD-PARTY-LICENSES.md"

    mkdir -p "$dependency_control_dir/debian"
    printf '%s\n' \
        'Source: better-os-packaging' \
        'Section: utils' \
        'Priority: optional' \
        "Maintainer: $MAINTAINER" \
        'Package: '"$package_name" \
        'Architecture: any' \
        'Depends: ${shlibs:Depends}' \
        'Description: Better OS packaging dependency scan' \
        ' Internal control file used to derive shared-library dependencies.' \
        > "$dependency_control_dir/debian/control"

    # Every binary in the package is scanned, not just the first one: a package
    # whose service links something its window does not would otherwise ship a
    # dependency nobody declared.
    shlib_dependencies="$(
        cd "$dependency_control_dir"
        dpkg-shlibdeps -O "${scanned_binaries[@]}" |
            sed -n 's/^shlibs:Depends=//p'
    )"
    if [[ -z "$shlib_dependencies" ]]; then
        printf 'Could not derive shared-library dependencies for %s\n' "$package_name" >&2
        exit 1
    fi

    local declared_dependencies="$shlib_dependencies"
    for entry in ${PACKAGE_DEPENDS[@]+"${PACKAGE_DEPENDS[@]}"}; do
        declared_dependencies="$declared_dependencies, $entry"
    done

    runtime_dependencies="$(
        printf '%s\n' "$declared_dependencies" |
            tr ',' '\n' |
            sed 's/^[[:space:]]*//; s/[[:space:]]*$//' |
            awk 'NF && !seen[$0]++ { if (out) out = out ", "; out = out $0 } END { print out }'
    )"

    printf '%s\n' \
        "Package: $package_name" \
        "Version: $VERSION" \
        'Section: utils' \
        'Priority: optional' \
        'Architecture: '"$ARCH" \
        "Maintainer: $MAINTAINER" \
        "Depends: $runtime_dependencies" \
        > "$staging_dir/DEBIAN/control"
    # Recommends, not Depends: the manager still runs without the privileged
    # service, it just reports that it cannot apply changes instead of
    # pretending to.
    if [[ -n "$PACKAGE_RECOMMENDS" ]]; then
        printf 'Recommends: %s\n' "$PACKAGE_RECOMMENDS" >> "$staging_dir/DEBIAN/control"
    fi
    printf '%s\n' \
        "Description: $description" \
        " $description_body" \
        >> "$staging_dir/DEBIAN/control"

    dpkg-deb --build --root-owner-group "$staging_dir" "$deb_path" >/dev/null
    (
        cd "$OUTPUT_DIR"
        sha256sum "$deb_filename"
    ) > "$deb_path.sha256"
    printf 'Built %s (%s, %s)\n' "$deb_path" "$VERSION" "$ARCH"
    printf 'Depends: %s\n' "$runtime_dependencies"
    reset_package_inputs
}

# The privileged service ships separately from the desktop applications. It is
# the only component that changes the system, so it gets its own package to
# review, version, and roll back on its own terms.
make_daemon_package() {
    local package_name="better-manager-daemon"
    local binary_name="better-manager-daemon"
    local staging_dir="$WORK_DIR/$package_name"
    local dependency_control_dir="$WORK_DIR/$package_name-deps"
    local binary_path="$BUILD_DIR/$binary_name"
    local deb_filename="${package_name}_${VERSION}_${RELEASE_TARGET}_${ARCH}.deb"
    local deb_path="$OUTPUT_DIR/$deb_filename"
    local shlib_dependencies
    local runtime_dependencies

    if [[ ! -x "$binary_path" ]]; then
        printf 'Missing release binary: %s\n' "$binary_path" >&2
        exit 1
    fi

    mkdir -p \
        "$staging_dir/DEBIAN" \
        "$staging_dir/usr/libexec" \
        "$staging_dir/usr/lib/systemd/system" \
        "$staging_dir/usr/share/dbus-1/system-services" \
        "$staging_dir/usr/share/dbus-1/system.d" \
        "$staging_dir/usr/share/polkit-1/actions" \
        "$staging_dir/usr/share/doc/$package_name"

    # /usr/libexec, not /usr/bin: this is started by D-Bus, not by a person.
    install -m 0755 "$binary_path" "$staging_dir/usr/libexec/$binary_name"
    install -m 0644 "$ROOT_DIR/packaging/daemon/better-manager-daemon.service" \
        "$staging_dir/usr/lib/systemd/system/better-manager-daemon.service"
    install -m 0644 "$ROOT_DIR/packaging/daemon/org.betteros.Manager1.service" \
        "$staging_dir/usr/share/dbus-1/system-services/org.betteros.Manager1.service"
    install -m 0644 "$ROOT_DIR/packaging/daemon/org.betteros.Manager1.conf" \
        "$staging_dir/usr/share/dbus-1/system.d/org.betteros.Manager1.conf"
    install -m 0644 "$ROOT_DIR/packaging/daemon/org.betteros.manager.policy" \
        "$staging_dir/usr/share/polkit-1/actions/org.betteros.manager.policy"
    install -m 0644 "$ROOT_DIR/LICENSE" "$staging_dir/usr/share/doc/$package_name/copyright"
    install -m 0644 "$ROOT_DIR/docs/third-party-licenses.md" \
        "$staging_dir/usr/share/doc/$package_name/THIRD-PARTY-LICENSES.md"

    mkdir -p "$dependency_control_dir/debian"
    printf '%s\n' \
        'Source: better-os-packaging' \
        'Section: admin' \
        'Priority: optional' \
        "Maintainer: $MAINTAINER" \
        'Package: '"$package_name" \
        'Architecture: any' \
        'Depends: ${shlibs:Depends}' \
        'Description: Better OS packaging dependency scan' \
        ' Internal control file used to derive shared-library dependencies.' \
        > "$dependency_control_dir/debian/control"

    shlib_dependencies="$(
        cd "$dependency_control_dir"
        dpkg-shlibdeps -O "$staging_dir/usr/libexec/$binary_name" |
            sed -n 's/^shlibs:Depends=//p'
    )"
    if [[ -z "$shlib_dependencies" ]]; then
        printf 'Could not derive shared-library dependencies for %s\n' "$package_name" >&2
        exit 1
    fi

    # The daemon needs no graphics libraries. What it does need is the policy
    # and transport it is authorized through, and the package manager it drives.
    runtime_dependencies="$(
        printf '%s\n' "$shlib_dependencies, dbus, policykit-1 | polkitd, apt, dpkg" |
            tr ',' '\n' |
            sed 's/^[[:space:]]*//; s/[[:space:]]*$//' |
            awk 'NF && !seen[$0]++ { if (out) out = out ", "; out = out $0 } END { print out }'
    )"

    printf '%s\n' \
        "Package: $package_name" \
        "Version: $VERSION" \
        'Section: admin' \
        'Priority: optional' \
        'Architecture: '"$ARCH" \
        "Maintainer: $MAINTAINER" \
        "Depends: $runtime_dependencies" \
        'Description: Better OS privileged component transaction service' \
        ' D-Bus system service that applies Better OS component transactions' \
        ' through local APT, authorized by polkit. It revalidates every plan' \
        ' against this host before acting on it.' \
        > "$staging_dir/DEBIAN/control"

    cat > "$staging_dir/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e

if [ "$1" = "configure" ]; then
    install -d -m 0755 -o root -g root /var/lib/better-os
    install -d -m 0755 -o root -g root /var/lib/better-os/transactions
    install -d -m 0755 -o root -g root /var/lib/better-os/rollback
    install -d -m 0755 -o root -g root /var/lib/better-os/installed
    install -d -m 0755 -o root -g root /var/cache/better-os
    install -d -m 0755 -o root -g root /var/cache/better-os/archives

    # Pick up the new unit and bus policy. Failure here is not fatal: the
    # daemon is D-Bus activated and will be started correctly after the next
    # reload or reboot.
    if [ -d /run/systemd/system ]; then
        systemctl daemon-reload >/dev/null 2>&1 || true
    fi
    if command -v dbus-send >/dev/null 2>&1; then
        dbus-send --system --type=method_call --dest=org.freedesktop.DBus \
            /org/freedesktop/DBus org.freedesktop.DBus.ReloadConfig \
            >/dev/null 2>&1 || true
    fi
fi
POSTINST

    cat > "$staging_dir/DEBIAN/prerm" <<'PRERM'
#!/bin/sh
set -e

if [ -d /run/systemd/system ]; then
    systemctl stop better-manager-daemon.service >/dev/null 2>&1 || true
fi
PRERM

    cat > "$staging_dir/DEBIAN/postrm" <<'POSTRM'
#!/bin/sh
set -e

if [ "$1" = "purge" ]; then
    # Transaction history and cached packages are only meaningful to this
    # service, so a purge takes them with it.
    rm -rf /var/lib/better-os /var/cache/better-os
fi

if [ -d /run/systemd/system ]; then
    systemctl daemon-reload >/dev/null 2>&1 || true
fi
POSTRM

    chmod 0755 \
        "$staging_dir/DEBIAN/postinst" \
        "$staging_dir/DEBIAN/prerm" \
        "$staging_dir/DEBIAN/postrm"

    dpkg-deb --build --root-owner-group "$staging_dir" "$deb_path" >/dev/null
    (
        cd "$OUTPUT_DIR"
        sha256sum "$deb_filename"
    ) > "$deb_path.sha256"
    printf 'Built %s (%s, %s)\n' "$deb_path" "$VERSION" "$ARCH"
    printf 'Depends: %s\n' "$runtime_dependencies"
}

PACKAGE_BINARIES=("manager-gui:usr/bin/better-manager")
PACKAGE_DEPENDS=("$GRAPHICS_DEPENDS")
PACKAGE_RECOMMENDS="better-manager-daemon (= $VERSION)"
make_package better-manager \
    'Better OS manager desktop application' \
    'Better OS desktop application built with the shared manager and monitor contracts.'

# The window, the session service, and the command line are one component and
# one version: the CLI speaks the service's own IPC contract, so shipping them
# apart would let a user hold two halves that disagree.
#
# /usr/bin/better-monitor stays the window, because that is what the published
# v0.1.0 package installed and what the manifest declares. The command line is
# installed as better-monitor-cli, which its own --help does not yet say. That
# collision is recorded in docs/tickets/36-component-packaging.md; resolving it
# means renaming one of the two, which is not a packaging change.
PACKAGE_BINARIES=(
    "monitor-gui:usr/bin/better-monitor"
    "better-monitor-service:usr/bin/better-monitor-service"
    "better-monitor:usr/bin/better-monitor-cli"
)
PACKAGE_DATA=("packaging/monitor/better-monitor.service:usr/lib/systemd/user/better-monitor.service")
PACKAGE_DEPENDS=("$GRAPHICS_DEPENDS")
make_package better-monitor \
    'Better OS monitor desktop application' \
    'Better OS desktop application built with the shared manager and monitor contracts.'

PACKAGE_BINARIES=("better-launcher:usr/bin/better-launcher")
PACKAGE_DATA=("packaging/launcher/better-launcher.desktop:usr/share/applications/better-launcher.desktop")
PACKAGE_DEPENDS=("$GRAPHICS_DEPENDS")
make_package better-launcher \
    'Better OS application launcher overlay' \
    'One overlay with the search row on top and the whole application library below it.'

PACKAGE_BINARIES=("better-files:usr/bin/better-files")
PACKAGE_DATA=("packaging/files/io.betteros.Files.desktop:usr/share/applications/io.betteros.Files.desktop")
# udisks2 is a Recommends rather than a Depends: without it Better Files still
# browses files, it just cannot mount or eject a device, and it says so instead
# of hiding the devices.
PACKAGE_DEPENDS=("$GRAPHICS_DEPENDS")
PACKAGE_RECOMMENDS="udisks2"
make_package better-files \
    'Better OS file manager' \
    'A file manager whose operations are durable jobs and whose devices say when they are safe to unplug.'

# The safe-mode entry point ships in the same package as the window it recovers
# from. A recovery path in a package a user has to install separately, after
# the desktop has already become hard to use, would not be a recovery path.
PACKAGE_BINARIES=("better-touchpad:usr/bin/better-touchpad")
PACKAGE_DATA=(
    "packaging/touchpad/better-touchpad.desktop:usr/share/applications/better-touchpad.desktop"
    "packaging/touchpad/better-touchpad-safe-mode.desktop:usr/share/applications/better-touchpad-safe-mode.desktop"
)
PACKAGE_DEPENDS=("$GRAPHICS_DEPENDS")
make_package better-touchpad \
    'Better OS touchpad settings' \
    'Scrolling, tapping, pointer speed, and gestures in one window, with every unavailable setting saying why.'

# The tray and the settings window are installed under the names the desktop
# entries and the component manifest already name, which are not the Cargo
# binary names. The unit is installed, not enabled: enabling is Better
# Manager's enable step, the same split better-manager-daemon uses.
PACKAGE_BINARIES=(
    "better-awake-service:usr/bin/better-awake-service"
    "better-awake-tray:usr/bin/awake-tray"
    "awake-gui:usr/bin/awake-gui"
)
PACKAGE_DATA=(
    "packaging/awake/better-awake.desktop:usr/share/applications/better-awake.desktop"
    "packaging/awake/better-awake-tray.desktop:etc/xdg/autostart/better-awake-tray.desktop"
    "packaging/awake/better-awake.service:usr/lib/systemd/user/better-awake.service"
)
PACKAGE_DEPENDS=("$GRAPHICS_DEPENDS")
make_package better-awake \
    'Better OS keep-awake sessions and rules' \
    'Keep-awake sessions and automatic rules, with every reason the machine is awake shown.'

# No window, so no graphics libraries — the same reasoning the privileged
# service is packaged with. udisks2 is a Depends and not a Recommends here:
# the service connects to UDisks2 before it owns its bus name, so without it
# there is no service at all.
PACKAGE_BINARIES=(
    "better-storage-service:usr/bin/better-storage-service"
    "better-storage-doctor:usr/bin/better-storage-doctor"
)
PACKAGE_DATA=(
    "packaging/storage/better-storage.service:usr/lib/systemd/user/better-storage.service"
    "packaging/storage/org.betteros.Storage1.service:usr/share/dbus-1/services/org.betteros.Storage1.service"
)
PACKAGE_DEPENDS=("dbus" "udisks2")
make_package better-storage \
    'Better OS external device removal service' \
    'Direct removal for external drives, with an honest ready-to-unplug state.'

make_daemon_package
