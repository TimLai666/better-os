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

# The privileged service is packaged differently from the desktop applications:
# it has no graphics dependencies, its binary lives in /usr/libexec because a
# person never starts it, and it is the only package that ships the policy and
# transport files that decide who may change this machine.
daemon_name="better-manager-daemon"
if [[ -n "$RELEASE_TARGET" ]]; then
    daemon_paths=("$DIST_DIR/${daemon_name}_"*"_${RELEASE_TARGET}_${EXPECTED_ARCH}.deb")
else
    daemon_paths=("$DIST_DIR/${daemon_name}_"*.deb)
fi
if [[ ${#daemon_paths[@]} -ne 1 ]]; then
    printf 'Expected exactly one target-specific package for %s\n' "$daemon_name" >&2
    exit 1
fi
daemon_path="${daemon_paths[0]}"
daemon_extract="$WORK_DIR/$daemon_name"

if [[ ! -f "$daemon_path" || ! -f "$daemon_path.sha256" ]]; then
    printf 'Missing package or checksum: %s\n' "$daemon_name" >&2
    exit 1
fi
(
    cd "$DIST_DIR"
    sha256sum --check "$(basename "$daemon_path.sha256")"
)

daemon_depends="$(dpkg-deb -f "$daemon_path" Depends)"
if [[ "$daemon_depends" =~ (^|,)[[:space:]]*[^,]*-dev([[:space:]]|,|$) ]]; then
    printf 'Build-time development package leaked into %s: %s\n' "$daemon_name" "$daemon_depends" >&2
    exit 1
fi
for required_dependency in dbus apt dpkg; do
    if ! printf '%s\n' "$daemon_depends" | grep -Eq "(^|,)[[:space:]]*${required_dependency}([[:space:]]|,|$)"; then
        printf 'Missing runtime dependency for %s: %s\n' "$daemon_name" "$required_dependency" >&2
        exit 1
    fi
done
if printf '%s\n' "$daemon_depends" | grep -Eq 'libwayland|libfontconfig|libxkbcommon'; then
    printf 'Graphics dependency leaked into the privileged service: %s\n' "$daemon_depends" >&2
    exit 1
fi

dpkg-deb --extract "$daemon_path" "$daemon_extract"
for required_file in \
    usr/libexec/better-manager-daemon \
    usr/lib/systemd/system/better-manager-daemon.service \
    usr/share/dbus-1/system-services/org.betteros.Manager1.service \
    usr/share/dbus-1/system-services/org.betteros.Monitor1.service \
    usr/share/dbus-1/system.d/org.betteros.Manager1.conf \
    usr/share/dbus-1/system.d/org.betteros.Monitor1.conf \
    usr/share/polkit-1/actions/org.betteros.manager.policy \
    usr/share/doc/better-manager-daemon/copyright \
    usr/share/doc/better-manager-daemon/THIRD-PARTY-LICENSES.md; do
    if [[ ! -s "$daemon_extract/$required_file" ]]; then
        printf 'Missing %s in %s\n' "$required_file" "$daemon_name" >&2
        exit 1
    fi
done
[[ -x "$daemon_extract/usr/libexec/better-manager-daemon" ]] || {
    printf 'The privileged service binary is not executable\n' >&2
    exit 1
}

# The unit must stay D-Bus activated. An [Install] section would mean it runs
# at boot, which is not what a package manager should do.
if grep -q '^\[Install\]' "$daemon_extract/usr/lib/systemd/system/better-manager-daemon.service"; then
    printf 'The privileged service must not be enabled at boot\n' >&2
    exit 1
fi
grep -q '^BusName=org.betteros.Manager1$' \
    "$daemon_extract/usr/lib/systemd/system/better-manager-daemon.service" || {
    printf 'The privileged service unit does not claim the expected bus name\n' >&2
    exit 1
}
grep -q 'org.betteros.manager.apply-transaction' \
    "$daemon_extract/usr/share/polkit-1/actions/org.betteros.manager.policy" || {
    printf 'The polkit policy does not declare the apply action\n' >&2
    exit 1
}
grep -q 'org.betteros.monitor.read-memory-devices' \
    "$daemon_extract/usr/share/polkit-1/actions/org.betteros.manager.policy" || {
    printf 'The polkit policy does not declare the monitor DMI action\n' >&2
    exit 1
}

if ldd "$daemon_extract/usr/libexec/better-manager-daemon" | grep -q 'not found'; then
    printf 'Unresolved dynamic library in %s\n' "$daemon_name" >&2
    exit 1
fi

printf 'Verified %s (%s)\n' "$daemon_path" "$EXPECTED_ARCH"
