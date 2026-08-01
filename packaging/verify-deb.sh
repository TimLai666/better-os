#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${1:-$ROOT_DIR/dist}"
RELEASE_TARGET="${2:-}"
EXPECTED_ARCH="$(dpkg --print-architecture)"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

if [[ -n "$RELEASE_TARGET" && ! "$RELEASE_TARGET" =~ ^[a-z0-9][a-z0-9.-]*$ ]]; then
    printf 'Invalid release target: %s\n' "$RELEASE_TARGET" >&2
    exit 1
fi

shopt -s nullglob

for package_name in better-manager better-monitor; do
    if [[ -n "$RELEASE_TARGET" ]]; then
        package_paths=("$DIST_DIR/${package_name}_"*"_${RELEASE_TARGET}_${EXPECTED_ARCH}.deb")
    else
        package_paths=("$DIST_DIR/${package_name}_"*.deb)
    fi
    if [[ ${#package_paths[@]} -ne 1 ]]; then
        printf 'Expected exactly one target-specific package for %s\n' "$package_name" >&2
        exit 1
    fi
    deb_path="${package_paths[0]}"
    checksum_path="$deb_path.sha256"
    extract_dir="$WORK_DIR/$package_name"

    if [[ ! -f "$deb_path" || ! -f "$checksum_path" ]]; then
        printf 'Missing package or checksum: %s\n' "$package_name" >&2
        exit 1
    fi

    (
        cd "$DIST_DIR"
        sha256sum --check "$(basename "$checksum_path")"
    )

    actual_name="$(dpkg-deb -f "$deb_path" Package)"
    actual_arch="$(dpkg-deb -f "$deb_path" Architecture)"
    depends="$(dpkg-deb -f "$deb_path" Depends)"

    [[ "$actual_name" == "$package_name" ]] || {
        printf 'Unexpected package name: %s\n' "$actual_name" >&2
        exit 1
    }
    [[ "$actual_arch" == "$EXPECTED_ARCH" ]] || {
        printf 'Unexpected architecture for %s: %s\n' "$package_name" "$actual_arch" >&2
        exit 1
    }

    if [[ "$depends" =~ (^|,)[[:space:]]*[^,]*-dev([[:space:]]|,|$) ]]; then
        printf 'Build-time development package leaked into %s: %s\n' "$package_name" "$depends" >&2
        exit 1
    fi

    for required_dependency in \
        libfontconfig1 \
        libxcb1 \
        libxkbcommon0 \
        libxkbcommon-x11-0 \
        libwayland-client0 \
        libwayland-egl1 \
        libwayland-cursor0; do
        if ! printf '%s\n' "$depends" | grep -Eq "(^|,)[[:space:]]*${required_dependency}([[:space:]]|,|$)"; then
            printf 'Missing runtime dependency for %s: %s\n' "$package_name" "$required_dependency" >&2
            exit 1
        fi
    done

    dpkg-deb --extract "$deb_path" "$extract_dir"
    binary_path="$extract_dir/usr/bin/$package_name"
    [[ -x "$binary_path" ]] || {
        printf 'Missing executable in %s\n' "$package_name" >&2
        exit 1
    }

    notice_dir="$extract_dir/usr/share/doc/$package_name"
    [[ -s "$notice_dir/copyright" ]] || {
        printf 'Missing project license notice in %s\n' "$package_name" >&2
        exit 1
    }
    [[ -s "$notice_dir/THIRD-PARTY-LICENSES.md" ]] || {
        printf 'Missing third-party license notice inventory in %s\n' "$package_name" >&2
        exit 1
    }
    cmp "$ROOT_DIR/LICENSE" "$notice_dir/copyright" >/dev/null || {
        printf 'Project license notice does not match repository LICENSE in %s\n' "$package_name" >&2
        exit 1
    }
    grep -q '^# Third-Party License Notices$' "$notice_dir/THIRD-PARTY-LICENSES.md" || {
        printf 'Invalid third-party license notice inventory in %s\n' "$package_name" >&2
        exit 1
    }

    if ldd "$binary_path" | grep -q 'not found'; then
        printf 'Unresolved dynamic library in %s\n' "$package_name" >&2
        exit 1
    fi

    printf 'Verified %s (%s)\n' "$deb_path" "$actual_arch"
done
