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
cargo build --release -p manager-gui -p monitor-gui -p manager-daemon

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
BUILD_DIR="$TARGET_DIR/release"
mkdir -p "$OUTPUT_DIR"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

make_package() {
    local package_name="$1"
    local binary_name="$2"
    local description="$3"
    local recommends="${4:-}"
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

    mkdir -p "$staging_dir/DEBIAN" "$staging_dir/usr/bin" "$staging_dir/usr/share/doc/$package_name"
    install -m 0755 "$binary_path" "$staging_dir/usr/bin/$package_name"
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

    shlib_dependencies="$(
        cd "$dependency_control_dir"
        dpkg-shlibdeps -O "$staging_dir/usr/bin/$package_name" |
            sed -n 's/^shlibs:Depends=//p'
    )"
    if [[ -z "$shlib_dependencies" ]]; then
        printf 'Could not derive shared-library dependencies for %s\n' "$package_name" >&2
        exit 1
    fi

    runtime_dependencies="$(
        printf '%s\n' "$shlib_dependencies, libfontconfig1, libwayland-client0, libwayland-egl1, libwayland-cursor0" |
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
    if [[ -n "$recommends" ]]; then
        printf 'Recommends: %s\n' "$recommends" >> "$staging_dir/DEBIAN/control"
    fi
    printf '%s\n' \
        "Description: $description" \
        ' Better OS desktop application built with the shared manager and monitor contracts.' \
        >> "$staging_dir/DEBIAN/control"

    dpkg-deb --build --root-owner-group "$staging_dir" "$deb_path" >/dev/null
    (
        cd "$OUTPUT_DIR"
        sha256sum "$deb_filename"
    ) > "$deb_path.sha256"
    printf 'Built %s (%s, %s)\n' "$deb_path" "$VERSION" "$ARCH"
    printf 'Depends: %s\n' "$runtime_dependencies"
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

make_package better-manager manager-gui 'Better OS manager desktop application' \
    "better-manager-daemon (= $VERSION)"
make_package better-monitor monitor-gui 'Better OS monitor desktop application'
make_daemon_package
