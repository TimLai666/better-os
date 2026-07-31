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

for command_name in cargo dpkg dpkg-deb dpkg-shlibdeps install sha256sum; do
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
cargo build --release -p manager-gui -p monitor-gui

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
BUILD_DIR="$TARGET_DIR/release"
mkdir -p "$OUTPUT_DIR"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

make_package() {
    local package_name="$1"
    local binary_name="$2"
    local description="$3"
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

    mkdir -p "$staging_dir/DEBIAN" "$staging_dir/usr/bin"
    install -m 0755 "$binary_path" "$staging_dir/usr/bin/$package_name"

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
        "Description: $description" \
        ' Better OS desktop application built with the shared manager and monitor contracts.' \
        > "$staging_dir/DEBIAN/control"

    dpkg-deb --build --root-owner-group "$staging_dir" "$deb_path" >/dev/null
    (
        cd "$OUTPUT_DIR"
        sha256sum "$deb_filename"
    ) > "$deb_path.sha256"
    printf 'Built %s (%s, %s)\n' "$deb_path" "$VERSION" "$ARCH"
    printf 'Depends: %s\n' "$runtime_dependencies"
}

make_package better-manager manager-gui 'Better OS manager desktop application'
make_package better-monitor monitor-gui 'Better OS monitor desktop application'
