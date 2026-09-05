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

# The uuid GNOME finds the adapter extension by. It has to match the directory
# the package installs it into and the uuid inside its own metadata; both are
# checked below, because a mismatch produces an extension the shell never sees
# rather than an error anybody would notice.
EXTENSION_UUID="touchpad-adapter@betteros.org"

# What each desktop package must actually contain. The list is here rather than
# inferred from the package name because most of these packages install more
# than one binary, and a package that quietly stopped shipping its service or
# its recovery entry point would otherwise still pass.
required_executables() {
    case "$1" in
        better-manager)
            printf '%s\n' usr/bin/better-manager
            ;;
        better-monitor)
            printf '%s\n' \
                usr/bin/better-monitor \
                usr/bin/better-monitor-service \
                usr/bin/better-monitor-cli
            ;;
        better-launcher)
            printf '%s\n' usr/bin/better-launcher
            ;;
        better-files)
            printf '%s\n' usr/bin/better-files
            ;;
        better-touchpad)
            printf '%s\n' \
                usr/bin/better-touchpad \
                usr/bin/better-touchpad-gestured
            ;;
        better-awake)
            printf '%s\n' \
                usr/bin/better-awake-service \
                usr/bin/awake-tray \
                usr/bin/awake-gui
            ;;
        *)
            printf 'No executable list for %s\n' "$1" >&2
            exit 1
            ;;
    esac
}

required_data_files() {
    case "$1" in
        better-manager)
            printf '%s\n' \
                usr/share/applications/io.betteros.Manager.desktop \
                usr/share/icons/hicolor/scalable/apps/better-manager.svg
            ;;
        better-monitor)
            printf '%s\n' \
                usr/lib/systemd/user/better-monitor.service \
                usr/share/applications/io.betteros.Monitor.desktop \
                usr/share/icons/hicolor/scalable/apps/better-monitor.svg
            ;;
        better-launcher)
            printf '%s\n' \
                usr/share/applications/better-launcher.desktop \
                usr/share/icons/hicolor/scalable/apps/better-launcher.svg
            ;;
        better-files)
            printf '%s\n' \
                usr/share/applications/io.betteros.Files.desktop \
                usr/share/icons/hicolor/scalable/apps/better-files.svg
            ;;
        better-touchpad)
            printf '%s\n' \
                usr/share/applications/better-touchpad.desktop \
                usr/share/applications/better-touchpad-safe-mode.desktop \
                usr/lib/systemd/user/better-touchpad-gestures.service \
                "usr/share/gnome-shell/extensions/$EXTENSION_UUID/metadata.json" \
                "usr/share/gnome-shell/extensions/$EXTENSION_UUID/extension.js" \
                "usr/share/gnome-shell/extensions/$EXTENSION_UUID/org.betteros.TouchpadAdapter1.xml" \
                usr/share/icons/hicolor/scalable/apps/better-touchpad.svg
            ;;
        better-awake)
            printf '%s\n' \
                usr/share/applications/better-awake.desktop \
                etc/xdg/autostart/better-awake-tray.desktop \
                usr/lib/systemd/user/better-awake.service \
                usr/share/icons/hicolor/scalable/apps/better-awake.svg
            ;;
        *)
            printf 'No data file list for %s\n' "$1" >&2
            exit 1
            ;;
    esac
}

# A package may ship a systemd user unit. It may not ship the symlink that
# enables one. Installing a component and turning it on are separate steps in
# every lifecycle this project has, and a .wants symlink in the payload would
# quietly collapse them into one.
assert_nothing_is_enabled_at_install() {
    local package_name="$1"
    local extract_dir="$2"
    local enablement_links=("$extract_dir"/usr/lib/systemd/user/*.wants/* "$extract_dir"/etc/systemd/user/*.wants/*)
    if ((${#enablement_links[@]} > 0)); then
        printf '%s enables a systemd user unit at install time: %s\n' \
            "$package_name" "${enablement_links[0]}" >&2
        exit 1
    fi
}

# desktop-file-validate is what says a shipped entry actually parses. It is not
# a build dependency of this project, so a host without it prints a note and the
# check is skipped rather than failing the whole verification — the same shape
# the e2e harness uses for polkitd, which it starts when it is there and does
# without when it is not.
DESKTOP_VALIDATE=1
if ! command -v desktop-file-validate >/dev/null 2>&1; then
    DESKTOP_VALIDATE=0
    printf 'desktop-file-validate is not installed: desktop entry validation skipped\n'
fi

assert_desktop_entries_are_valid() {
    local package_name="$1"
    local extract_dir="$2"
    local entry icon_name
    local entries=(
        "$extract_dir"/usr/share/applications/*.desktop
        "$extract_dir"/etc/xdg/autostart/*.desktop
    )

    for entry in ${entries[@]+"${entries[@]}"}; do
        if ((DESKTOP_VALIDATE)); then
            desktop-file-validate "$entry" || {
                printf 'Invalid desktop entry in %s: %s\n' "$package_name" "$entry" >&2
                exit 1
            }
        fi
        # Icon= is a theme name, not a path, so an entry that names an icon
        # nobody ships is still a valid entry — it just draws a blank tile in
        # the applications grid, which is what every Better OS entry did until
        # the icons were packaged. Every name a shipped entry uses has to have
        # a file behind it in the same package.
        icon_name="$(sed -n 's/^Icon=//p' "$entry")"
        if [[ -n "$icon_name" ]]; then
            [[ -s "$extract_dir/usr/share/icons/hicolor/scalable/apps/$icon_name.svg" ]] || {
                printf 'Desktop entry %s in %s names an icon the package does not ship: %s\n' \
                    "$(basename "$entry")" "$package_name" "$icon_name" >&2
                exit 1
            }
        fi
    done
}

for package_name in \
    better-manager \
    better-monitor \
    better-launcher \
    better-files \
    better-touchpad \
    better-awake; do
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

    executables=()
    while IFS= read -r relative_path; do
        executables+=("$relative_path")
    done < <(required_executables "$package_name")

    for relative_path in "${executables[@]}"; do
        [[ -x "$extract_dir/$relative_path" ]] || {
            printf 'Missing executable %s in %s\n' "$relative_path" "$package_name" >&2
            exit 1
        }
    done

    while IFS= read -r relative_path; do
        [[ -s "$extract_dir/$relative_path" ]] || {
            printf 'Missing %s in %s\n' "$relative_path" "$package_name" >&2
            exit 1
        }
    done < <(required_data_files "$package_name")

    assert_nothing_is_enabled_at_install "$package_name" "$extract_dir"
    assert_desktop_entries_are_valid "$package_name" "$extract_dir"

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

    for relative_path in "${executables[@]}"; do
        if ldd "$extract_dir/$relative_path" | grep -q 'not found'; then
            printf 'Unresolved dynamic library in %s: %s\n' "$package_name" "$relative_path" >&2
            exit 1
        fi
    done

    # The claims each package's own payload has to keep. These are the lines a
    # user or another component relies on, not a restatement of the file list.
    case "$package_name" in
        better-manager)
            grep -q '^Exec=better-manager$' \
                "$extract_dir/usr/share/applications/io.betteros.Manager.desktop" || {
                printf 'The Better Manager desktop entry does not run the packaged window\n' >&2
                exit 1
            }
            ;;
        better-launcher)
            # Clicking a launcher icon opens the launcher. It must never be the
            # thing that closes it, which is what a bare Exec would do.
            grep -q '^Exec=better-launcher --open$' \
                "$extract_dir/usr/share/applications/better-launcher.desktop" || {
                printf 'The launcher desktop entry does not open the overlay\n' >&2
                exit 1
            }
            ;;
        better-files)
            grep -q '^Exec=better-files' \
                "$extract_dir/usr/share/applications/io.betteros.Files.desktop" || {
                printf 'The Better Files desktop entry does not run the packaged binary\n' >&2
                exit 1
            }
            ;;
        better-touchpad)
            # The recovery entry point has to work when the configuration is
            # what broke the desktop, so it ships beside the window rather than
            # in a package the user would have to install after the fact.
            grep -q '^Exec=better-touchpad --safe-mode$' \
                "$extract_dir/usr/share/applications/better-touchpad-safe-mode.desktop" || {
                printf 'The safe-mode desktop entry does not enter safe mode\n' >&2
                exit 1
            }
            grep -q '^ExecStart=/usr/bin/better-touchpad-gestured$' \
                "$extract_dir/usr/lib/systemd/user/better-touchpad-gestures.service" || {
                printf 'The gesture user unit does not start the packaged service\n' >&2
                exit 1
            }
            # GNOME finds an extension by the uuid in its metadata matching the
            # directory it is installed in. A mismatch installs a directory the
            # shell will never look at, and nothing else would catch it.
            grep -q "\"uuid\": \"$EXTENSION_UUID\"" \
                "$extract_dir/usr/share/gnome-shell/extensions/$EXTENSION_UUID/metadata.json" || {
                printf 'The shell extension uuid does not match its install directory\n' >&2
                exit 1
            }
            # The extension reads its D-Bus contract from the file beside it, so
            # a package that shipped one without the other would export nothing.
            grep -q 'org.betteros.TouchpadAdapter1.xml' \
                "$extract_dir/usr/share/gnome-shell/extensions/$EXTENSION_UUID/extension.js" || {
                printf 'The packaged extension does not load its interface file\n' >&2
                exit 1
            }
            grep -q '<interface name="org.betteros.TouchpadAdapter1">' \
                "$extract_dir/usr/share/gnome-shell/extensions/$EXTENSION_UUID/org.betteros.TouchpadAdapter1.xml" || {
                printf 'The packaged interface file does not declare the adapter interface\n' >&2
                exit 1
            }
            ;;
        better-awake)
            # The tray is an indicator for the service, not an application a
            # user launches out of the menu.
            grep -q '^NoDisplay=true$' \
                "$extract_dir/etc/xdg/autostart/better-awake-tray.desktop" || {
                printf 'The Better Awake tray autostart entry is not hidden from the menu\n' >&2
                exit 1
            }
            grep -q '^ExecStart=/usr/bin/better-awake-service$' \
                "$extract_dir/usr/lib/systemd/user/better-awake.service" || {
                printf 'The Better Awake user unit does not start the packaged service\n' >&2
                exit 1
            }
            ;;
        better-monitor)
            grep -q '^ExecStart=/usr/bin/better-monitor-service$' \
                "$extract_dir/usr/lib/systemd/user/better-monitor.service" || {
                printf 'The Better Monitor user unit does not start the packaged service\n' >&2
                exit 1
            }
            # The package ships three executables and only one of them has a
            # window. An entry that launched the service or the command line
            # would put a menu item in the grid that opens nothing.
            grep -q '^Exec=better-monitor$' \
                "$extract_dir/usr/share/applications/io.betteros.Monitor.desktop" || {
                printf 'The Better Monitor desktop entry does not run the window binary\n' >&2
                exit 1
            }
            ;;
    esac

    printf 'Verified %s (%s)\n' "$deb_path" "$actual_arch"
done

# Better Storage has no window, so it is verified the way the privileged
# service is rather than with the desktop packages: no graphics dependencies,
# a session unit, and the D-Bus activation file its clients reach it through.
storage_name="better-storage"
if [[ -n "$RELEASE_TARGET" ]]; then
    storage_paths=("$DIST_DIR/${storage_name}_"*"_${RELEASE_TARGET}_${EXPECTED_ARCH}.deb")
else
    storage_paths=("$DIST_DIR/${storage_name}_"*.deb)
fi
if [[ ${#storage_paths[@]} -ne 1 ]]; then
    printf 'Expected exactly one target-specific package for %s\n' "$storage_name" >&2
    exit 1
fi
storage_path="${storage_paths[0]}"
storage_extract="$WORK_DIR/$storage_name"

if [[ ! -f "$storage_path" || ! -f "$storage_path.sha256" ]]; then
    printf 'Missing package or checksum: %s\n' "$storage_name" >&2
    exit 1
fi
(
    cd "$DIST_DIR"
    sha256sum --check "$(basename "$storage_path.sha256")"
)

storage_depends="$(dpkg-deb -f "$storage_path" Depends)"
if [[ "$storage_depends" =~ (^|,)[[:space:]]*[^,]*-dev([[:space:]]|,|$) ]]; then
    printf 'Build-time development package leaked into %s: %s\n' "$storage_name" "$storage_depends" >&2
    exit 1
fi
# UDisks2 is a hard dependency, not a recommendation: the service connects to
# it before it owns its bus name, so without it there is no service at all.
for required_dependency in dbus udisks2; do
    if ! printf '%s\n' "$storage_depends" | grep -Eq "(^|,)[[:space:]]*${required_dependency}([[:space:]]|,|$)"; then
        printf 'Missing runtime dependency for %s: %s\n' "$storage_name" "$required_dependency" >&2
        exit 1
    fi
done
if printf '%s\n' "$storage_depends" | grep -Eq 'libwayland|libfontconfig|libxkbcommon'; then
    printf 'Graphics dependency leaked into a package with no window: %s\n' "$storage_depends" >&2
    exit 1
fi

dpkg-deb --extract "$storage_path" "$storage_extract"
for required_file in \
    usr/bin/better-storage-service \
    usr/bin/better-storage-doctor \
    usr/lib/systemd/user/better-storage.service \
    usr/share/dbus-1/services/org.betteros.Storage1.service \
    usr/share/doc/better-storage/copyright \
    usr/share/doc/better-storage/THIRD-PARTY-LICENSES.md; do
    if [[ ! -s "$storage_extract/$required_file" ]]; then
        printf 'Missing %s in %s\n' "$required_file" "$storage_name" >&2
        exit 1
    fi
done
cmp "$ROOT_DIR/LICENSE" "$storage_extract/usr/share/doc/better-storage/copyright" >/dev/null || {
    printf 'Project license notice does not match repository LICENSE in %s\n' "$storage_name" >&2
    exit 1
}
grep -q '^# Third-Party License Notices$' \
    "$storage_extract/usr/share/doc/better-storage/THIRD-PARTY-LICENSES.md" || {
    printf 'Invalid third-party license notice inventory in %s\n' "$storage_name" >&2
    exit 1
}
for required_executable in \
    usr/bin/better-storage-service \
    usr/bin/better-storage-doctor; do
    [[ -x "$storage_extract/$required_executable" ]] || {
        printf '%s is not executable in %s\n' "$required_executable" "$storage_name" >&2
        exit 1
    }
    if ldd "$storage_extract/$required_executable" | grep -q 'not found'; then
        printf 'Unresolved dynamic library in %s: %s\n' "$storage_name" "$required_executable" >&2
        exit 1
    fi
done
grep -q '^Name=org.betteros.Storage1$' \
    "$storage_extract/usr/share/dbus-1/services/org.betteros.Storage1.service" || {
    printf 'The storage D-Bus activation file does not claim the expected bus name\n' >&2
    exit 1
}
grep -q '^ExecStart=/usr/bin/better-storage-service$' \
    "$storage_extract/usr/lib/systemd/user/better-storage.service" || {
    printf 'The Better Storage user unit does not start the packaged service\n' >&2
    exit 1
}
assert_nothing_is_enabled_at_install "$storage_name" "$storage_extract"

printf 'Verified %s (%s)\n' "$storage_path" "$EXPECTED_ARCH"

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
    usr/share/dbus-1/system.d/org.betteros.Manager1.conf \
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

if ldd "$daemon_extract/usr/libexec/better-manager-daemon" | grep -q 'not found'; then
    printf 'Unresolved dynamic library in %s\n' "$daemon_name" >&2
    exit 1
fi

printf 'Verified %s (%s)\n' "$daemon_path" "$EXPECTED_ARCH"
