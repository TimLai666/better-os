#!/usr/bin/env bash
# Better OS bootstrap installer.
#
# Installs Better Manager and the privileged service it needs, from the
# published GitHub Release, with every package checksum-verified before
# anything is handed to apt. Everything else Better OS ships is installed
# from inside Better Manager.
#
# This script is meant to be downloaded and then run, not piped into a shell:
#
#   curl -fsSL -o /tmp/better-os-install.sh \
#     https://raw.githubusercontent.com/TimLai666/better-os/main/install.sh
#   bash /tmp/better-os-install.sh
#
# It asks for sudo exactly once, and prints the command it will run as root
# before asking.
set -euo pipefail

REPOSITORY="TimLai666/better-os"
API_BASE="https://api.github.com/repos/$REPOSITORY"

# The two packages this installer is responsible for. Better Manager is the
# application; the daemon is the only thing on the machine allowed to change
# package state, so the manager is useless without it. Everything else is
# installed from inside Better Manager rather than from here.
PACKAGES=(better-manager better-manager-daemon)

MODE="install"
DRY_RUN=0
FROM_DIR=""

# Filled in by the detection and resolution steps.
UBUNTU_RELEASE=""
ARCHITECTURE=""
RELEASE_TAG=""
RELEASE_VERSION=""
WORK_DIR=""

info() {
    printf '%s\n' "$*"
}

step() {
    printf '\n== %s ==\n' "$*"
}

fail() {
    printf 'better-os install: %s\n' "$*" >&2
    exit 1
}

cleanup() {
    if [[ -n "$WORK_DIR" && -d "$WORK_DIR" ]]; then
        rm -rf "$WORK_DIR"
    fi
}
trap cleanup EXIT

usage() {
    cat <<'USAGE'
Usage: install.sh [options]

Installs Better Manager and better-manager-daemon from the latest public
Better OS release, verifying each package checksum before installing.

Options:
  --dry-run          Print everything that would happen and change nothing.
  --uninstall        Remove the packages this installer installs.
  --from-dir DIR     Install from locally built packages in DIR instead of
                     downloading a release. Used by the project's own tests;
                     checksum verification still applies.
  -h, --help         Show this message.

Environment:
  GITHUB_TOKEN       Optional. Sent as a bearer token to the GitHub API only,
                     to raise the anonymous rate limit. Never required, and
                     never sent anywhere else.
USAGE
}

parse_arguments() {
    while (($# > 0)); do
        case "$1" in
            --dry-run)
                DRY_RUN=1
                shift
                ;;
            --uninstall)
                MODE="uninstall"
                shift
                ;;
            --from-dir)
                if (($# < 2)); then
                    usage >&2
                    exit 2
                fi
                FROM_DIR="$2"
                shift 2
                ;;
            --from-dir=*)
                FROM_DIR="${1#--from-dir=}"
                shift
                ;;
            -h | --help)
                usage
                exit 0
                ;;
            *)
                printf 'Unknown option: %s\n\n' "$1" >&2
                usage >&2
                exit 2
                ;;
        esac
    done

    if [[ -n "$FROM_DIR" && "$MODE" == "uninstall" ]]; then
        fail "--from-dir has no meaning with --uninstall."
    fi
}

require_commands() {
    local missing=()
    local command_name
    for command_name in "$@"; do
        if ! command -v "$command_name" >/dev/null 2>&1; then
            missing+=("$command_name")
        fi
    done
    if ((${#missing[@]} > 0)); then
        fail "missing required command(s): ${missing[*]}"
    fi
}

# Which Ubuntu release the packages have to be built against. Derivatives are
# the reason this is not a one-line read of VERSION_ID: Zorin OS 18 reports
# VERSION_ID="18", which names nothing in the release matrix, and the only
# field that says what it is built on is UBUNTU_CODENAME=noble. The codename
# is therefore preferred over the version wherever one exists, and VERSION_ID
# is the fallback for plain Ubuntu.
detect_ubuntu_release() {
    # The path is a variable so the mapping can be exercised against a fixture
    # for a release this machine is not running. It only chooses which
    # published package is fetched, so overriding it buys nothing a person
    # could not get by downloading the other package by hand.
    local os_release="${BETTER_OS_INSTALL_OS_RELEASE:-/etc/os-release}"
    [[ -r "$os_release" ]] || fail "cannot read $os_release, so this system cannot be identified."

    local distribution_id="" version_id="" ubuntu_codename="" version_codename="" pretty_name=""
    distribution_id="$(read_os_release_field ID "$os_release")"
    version_id="$(read_os_release_field VERSION_ID "$os_release")"
    ubuntu_codename="$(read_os_release_field UBUNTU_CODENAME "$os_release")"
    version_codename="$(read_os_release_field VERSION_CODENAME "$os_release")"
    pretty_name="$(read_os_release_field PRETTY_NAME "$os_release")"
    [[ -n "$pretty_name" ]] || pretty_name="${distribution_id:-unknown system}"

    local codename="$ubuntu_codename"
    if [[ -z "$codename" && "$distribution_id" == "ubuntu" ]]; then
        codename="$version_codename"
    fi

    case "$codename" in
        jammy) UBUNTU_RELEASE="22.04" ;;
        noble) UBUNTU_RELEASE="24.04" ;;
        "")
            # No codename at all. Only plain Ubuntu is trusted to have a
            # VERSION_ID that names a release in the matrix; a derivative's
            # version is its own, not the base it was built from.
            if [[ "$distribution_id" == "ubuntu" ]]; then
                case "$version_id" in
                    22.04 | 24.04) UBUNTU_RELEASE="$version_id" ;;
                esac
            fi
            ;;
    esac

    if [[ -z "$UBUNTU_RELEASE" ]]; then
        cat >&2 <<UNSUPPORTED
better-os install: unsupported system.

Detected: $pretty_name (ID=${distribution_id:-unknown}, VERSION_ID=${version_id:-unknown},
UBUNTU_CODENAME=${ubuntu_codename:-unset}, VERSION_CODENAME=${version_codename:-unset})

Better OS publishes packages for Ubuntu 22.04 (jammy) and Ubuntu 24.04
(noble), and for derivatives built on them — including Zorin OS, which is
identified by its UBUNTU_CODENAME rather than its own version number.
UNSUPPORTED
        exit 1
    fi

    info "System: $pretty_name, installing the Ubuntu $UBUNTU_RELEASE packages."
}

# os-release is shell-syntax, but sourcing it would run whatever it contains.
# Read the one field instead, and strip the optional quotes ourselves.
read_os_release_field() {
    local field="$1"
    local path="$2"
    sed -n "s/^${field}=\"\{0,1\}\([^\"]*\)\"\{0,1\}$/\1/p" "$path" | head -n 1
}

detect_architecture() {
    ARCHITECTURE="$(dpkg --print-architecture)"
    case "$ARCHITECTURE" in
        amd64 | arm64) ;;
        *)
            fail "unsupported architecture: $ARCHITECTURE. Better OS publishes amd64 and arm64 packages."
            ;;
    esac
    info "Architecture: $ARCHITECTURE."
}

asset_name() {
    printf '%s_%s_ubuntu-%s_%s.deb\n' "$1" "$RELEASE_VERSION" "$UBUNTU_RELEASE" "$ARCHITECTURE"
}

# The latest release, from the public API, without gh and without a token.
# jq is used when it is there and is not a dependency: the two fields needed
# are a flat string and a list of flat strings, which grep and sed can read
# from GitHub's own formatting without pretending to parse JSON in general.
resolve_latest_release() {
    require_commands curl

    local response="$WORK_DIR/release.json"
    local http_status=""
    local curl_arguments=(
        --silent
        --show-error
        --location
        --retry 2
        --max-time 60
        --header 'Accept: application/vnd.github+json'
        --output "$response"
        --write-out '%{http_code}'
    )
    if [[ -n "${GITHUB_TOKEN:-}" ]]; then
        curl_arguments+=(--header "Authorization: Bearer $GITHUB_TOKEN")
    fi

    if ! http_status="$(curl "${curl_arguments[@]}" "$API_BASE/releases/latest")"; then
        fail "could not reach the GitHub API. Check the network connection and try again."
    fi

    case "$http_status" in
        200) ;;
        403 | 429)
            cat >&2 <<'RATELIMITED'
better-os install: the GitHub API refused the request, which on an anonymous
call almost always means the rate limit for this network address is used up.

Wait an hour and run this again, or set GITHUB_TOKEN to any personal access
token (no scopes needed) to use the higher authenticated limit:

  GITHUB_TOKEN=... bash install.sh

Or download the packages by hand from
https://github.com/TimLai666/better-os/releases/latest
RATELIMITED
            exit 1
            ;;
        401)
            fail "the GitHub API rejected GITHUB_TOKEN. Unset it — no token is needed — or set a valid one."
            ;;
        404)
            fail "the repository has no published release yet ($API_BASE/releases/latest returned 404)."
            ;;
        *)
            fail "the GitHub API returned HTTP $http_status. Try again later."
            ;;
    esac

    RELEASE_TAG="$(read_json_string tag_name "$response")"
    [[ -n "$RELEASE_TAG" ]] || fail "the GitHub API response carried no release tag."
    RELEASE_VERSION="${RELEASE_TAG#v}"
    if [[ ! "$RELEASE_VERSION" =~ ^[0-9][0-9A-Za-z.+~-]*$ ]]; then
        fail "the release tag '$RELEASE_TAG' does not look like a version."
    fi

    info "Latest release: $RELEASE_TAG"
}

# One string field out of a flat JSON object. jq when it exists, and a
# deliberately narrow grep otherwise: the field is matched with its quotes and
# colon so a value containing the field name cannot be mistaken for it.
read_json_string() {
    local field="$1"
    local path="$2"
    if command -v jq >/dev/null 2>&1; then
        jq -r --arg field "$field" '.[$field] // empty' "$path"
        return
    fi
    # A field that is not there is an empty answer, not a failure: the caller
    # decides whether its absence is fatal.
    grep -o "\"$field\"[[:space:]]*:[[:space:]]*\"[^\"]*\"" "$path" |
        head -n 1 |
        sed 's/.*:[[:space:]]*"\(.*\)"$/\1/' || true
}

# The download URL GitHub published for one asset name. Looking the URL up
# rather than composing it means a release that does not carry this system's
# package is reported as missing instead of producing a 404 mid-download.
asset_download_url() {
    local name="$1"
    local path="$2"
    if command -v jq >/dev/null 2>&1; then
        jq -r --arg name "$name" \
            '.assets[]? | select(.name == $name) | .browser_download_url' "$path" |
            head -n 1
        return
    fi
    # Without jq, the asset URL is found by its own shape rather than by
    # walking the object: every asset URL ends in the asset's file name, and
    # the name is target-specific, so the match is unambiguous.
    grep -o "\"browser_download_url\"[[:space:]]*:[[:space:]]*\"[^\"]*/$name\"" "$path" |
        head -n 1 |
        sed 's/.*:[[:space:]]*"\(.*\)"$/\1/' || true
}

installed_version() {
    local package="$1"
    local status=""
    status="$(dpkg-query -W -f='${db:Status-Status} ${Version}' "$package" 2>/dev/null || true)"
    case "$status" in
        "installed "*) printf '%s\n' "${status#installed }" ;;
        *) printf '\n' ;;
    esac
}

verify_checksum() {
    local deb_path="$1"
    local checksum_path="$2"
    local expected=""
    local actual=""

    expected="$(awk 'NR == 1 { print $1 }' "$checksum_path")"
    if [[ ! "$expected" =~ ^[0-9a-f]{64}$ ]]; then
        fail "the checksum sidecar for $(basename "$deb_path") is not a sha256 sum."
    fi
    actual="$(sha256sum "$deb_path" | awk '{print $1}')"
    if [[ "$actual" != "$expected" ]]; then
        fail "checksum mismatch for $(basename "$deb_path"): expected $expected, got $actual. Nothing was installed."
    fi
    info "  verified $(basename "$deb_path")"
}

# Copy the locally built packages into the same work directory a download
# would have filled, so the verify and install steps have one input shape.
stage_from_directory() {
    [[ -d "$FROM_DIR" ]] || fail "--from-dir: $FROM_DIR is not a directory."

    local package deb_path
    local -a matches
    shopt -s nullglob
    for package in "${PACKAGES[@]}"; do
        matches=("$FROM_DIR/${package}_"*"_ubuntu-${UBUNTU_RELEASE}_${ARCHITECTURE}.deb")
        if ((${#matches[@]} != 1)); then
            fail "--from-dir: expected exactly one ${package} package for ubuntu-${UBUNTU_RELEASE}/${ARCHITECTURE} in $FROM_DIR, found ${#matches[@]}."
        fi
        deb_path="${matches[0]}"
        [[ -f "$deb_path.sha256" ]] || fail "--from-dir: $deb_path has no .sha256 sidecar."
        if [[ -z "$RELEASE_VERSION" ]]; then
            local base="${deb_path##*/}"
            base="${base#"${package}"_}"
            RELEASE_VERSION="${base%%_*}"
            RELEASE_TAG="v$RELEASE_VERSION"
        fi
        cp "$deb_path" "$WORK_DIR/$(basename "$deb_path")"
        cp "$deb_path.sha256" "$WORK_DIR/$(basename "$deb_path").sha256"
    done
    shopt -u nullglob

    [[ -n "$RELEASE_VERSION" ]] || fail "--from-dir: could not read a version out of the package file names."
    info "Local packages: version $RELEASE_VERSION from $FROM_DIR"
}

download_packages() {
    local response="$WORK_DIR/release.json"
    local package name url checksum_url
    for package in "${PACKAGES[@]}"; do
        name="$(asset_name "$package")"
        url="$(asset_download_url "$name" "$response")"
        [[ -n "$url" ]] || fail "release $RELEASE_TAG carries no asset named $name."
        checksum_url="$(asset_download_url "$name.sha256" "$response")"
        [[ -n "$checksum_url" ]] || fail "release $RELEASE_TAG carries no checksum sidecar named $name.sha256."
        info "  downloading $name"
        curl --fail --silent --show-error --location --retry 2 \
            --output "$WORK_DIR/$name" "$url"
        info "  downloading $name.sha256"
        curl --fail --silent --show-error --location --retry 2 \
            --output "$WORK_DIR/$name.sha256" "$checksum_url"
    done
}

print_release_plan() {
    local response="$WORK_DIR/release.json"
    local package name url checksum_url
    for package in "${PACKAGES[@]}"; do
        name="$(asset_name "$package")"
        url="$(asset_download_url "$name" "$response")"
        [[ -n "$url" ]] || fail "release $RELEASE_TAG carries no asset named $name."
        checksum_url="$(asset_download_url "$name.sha256" "$response")"
        [[ -n "$checksum_url" ]] || fail "release $RELEASE_TAG carries no checksum sidecar named $name.sha256."
        info "  $name"
        info "    $url"
        info "    $checksum_url"
    done
}

# Everything that runs with root privileges, in one place, so the statement
# printed before asking for a password is the command itself rather than a
# description of it that could drift from it.
privileged_command() {
    case "$MODE" in
        install)
            local package
            printf '%s\n' apt-get install -y --no-install-recommends
            for package in "${PACKAGES[@]}"; do
                printf '%s\n' "$WORK_DIR/$(asset_name "$package")"
            done
            ;;
        uninstall)
            printf '%s\n' apt-get remove -y "${PACKAGES[@]}"
            ;;
    esac
}

run_privileged() {
    local argument
    local command_line=()
    while IFS= read -r argument; do
        command_line+=("$argument")
    done < <(privileged_command)

    step "what runs as root"
    if ((EUID == 0)); then
        info "You are already root, so nothing is asked for and this runs directly:"
    else
        info "This is the only command that needs root, and sudo is asked for once:"
    fi
    info ""
    info "  DEBIAN_FRONTEND=noninteractive ${command_line[*]}"
    info ""

    if ((DRY_RUN == 1)); then
        info "--dry-run: not run."
        return 0
    fi

    if ((EUID == 0)); then
        DEBIAN_FRONTEND=noninteractive "${command_line[@]}"
    else
        require_commands sudo
        sudo env DEBIAN_FRONTEND=noninteractive "${command_line[@]}"
    fi
}

report_installed_state() {
    local package version
    for package in "${PACKAGES[@]}"; do
        version="$(installed_version "$package")"
        if [[ -n "$version" ]]; then
            info "  $package $version"
        else
            info "  $package is not installed"
        fi
    done
}

do_install() {
    require_commands dpkg dpkg-query sha256sum apt-get
    detect_ubuntu_release
    detect_architecture

    WORK_DIR="$(mktemp -d)"

    step "resolving packages"
    if [[ -n "$FROM_DIR" ]]; then
        stage_from_directory
    else
        resolve_latest_release
    fi

    step "what is installed now"
    report_installed_state

    # Idempotence: a second run of an unchanged release has nothing to do, and
    # says so instead of asking for a password to reinstall the same files.
    local package current up_to_date=1
    for package in "${PACKAGES[@]}"; do
        current="$(installed_version "$package")"
        if [[ "$current" != "$RELEASE_VERSION" ]]; then
            up_to_date=0
        fi
    done
    if ((up_to_date == 1)); then
        step "nothing to do"
        info "Better Manager $RELEASE_VERSION is already installed and current."
        info "Open it from the applications menu, or run: better-manager"
        return 0
    fi

    if [[ -n "$FROM_DIR" ]]; then
        step "verifying checksums"
        for package in "${PACKAGES[@]}"; do
            verify_checksum "$WORK_DIR/$(asset_name "$package")" \
                "$WORK_DIR/$(asset_name "$package").sha256"
        done
    elif ((DRY_RUN == 1)); then
        step "packages that would be downloaded and verified"
        print_release_plan
        info ""
        info "--dry-run: nothing is downloaded, so nothing is verified here."
    else
        step "downloading $RELEASE_TAG"
        download_packages
        step "verifying checksums"
        for package in "${PACKAGES[@]}"; do
            verify_checksum "$WORK_DIR/$(asset_name "$package")" \
                "$WORK_DIR/$(asset_name "$package").sha256"
        done
        info "Both packages match their published checksums."
    fi

    run_privileged

    if ((DRY_RUN == 1)); then
        return 0
    fi

    step "installed"
    report_installed_state
    info ""
    info "Open Better Manager from the applications menu, or run: better-manager"
    info "Everything else Better OS ships is installed from inside it."
}

do_uninstall() {
    require_commands dpkg-query apt-get

    step "what is installed now"
    report_installed_state

    local package anything=0
    for package in "${PACKAGES[@]}"; do
        if [[ -n "$(installed_version "$package")" ]]; then
            anything=1
        fi
    done
    if ((anything == 0)); then
        step "nothing to do"
        info "Neither package this installer installs is present."
        return 0
    fi

    run_privileged

    if ((DRY_RUN == 1)); then
        return 0
    fi

    step "removed"
    report_installed_state
    info ""
    info "Components installed from inside Better Manager are not touched by this."
}

main() {
    parse_arguments "$@"
    printf 'Better OS installer\n'
    case "$MODE" in
        install) do_install ;;
        uninstall) do_uninstall ;;
    esac
}

main "$@"
