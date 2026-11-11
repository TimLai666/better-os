#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
TARGET_ARG=""
APPLY=0
OLDER_THAN_DAYS=7
CARGO_TAG_SIGNATURE='Signature: 8a477f597d28d172789f06886806bc55'

usage() {
    printf 'Usage: %s [--apply] [--older-than DAYS] [TARGET_DIR]\n' "$0"
}

print_help() {
    usage
    cat <<'EOF'

Frees the parts of a cargo target directory that the build in use does not
need. Without --apply it only lists what it would remove and how much space
that is, and removes nothing.

It removes:
  - incremental compilation caches (every profile's incremental/ directory);
  - other target directories nested inside this one, whole: any directory one
    or two levels below TARGET_DIR with its own cargo CACHEDIR.TAG, such as
    target/msrv-check or target/clippy-198. Whoever builds into one next
    builds from scratch;
  - release artifacts not modified in DAYS days: entries of release/deps,
    release/build, release/.fingerprint and release/examples, and the files at
    the top of release/, for target/release and target/<triple>/release. A
    build/ or .fingerprint/ entry with anything newer inside it is kept.

It keeps everything else, so the next debug `cargo build` compiles only what
changed: debug/deps, debug/build, debug/.fingerprint, the debug binaries,
cargo's lock files, doc/, tmp/, and any directory whose CACHEDIR.TAG is not
cargo's. To remove the whole target directory, use `cargo clean`.

Do not run it while cargo is building into TARGET_DIR.

Arguments:
  TARGET_DIR          Cargo target directory to prune.
                      default: target/ in the repository root
                      (the checkout this script is in).
                      It must hold a CACHEDIR.TAG written by cargo. The
                      filesystem root, the home directory, the repository
                      root, and any directory containing one of them are
                      refused. A symlink is followed and judged by where it
                      points.

Options:
  --apply             Remove what is listed. Without it nothing is removed.
  --older-than DAYS   Age, in whole days since last modification, beyond
                      which a release artifact is removed. 0 removes every
                      release artifact. default: 7
  --help              Show this help and exit.

Sizes come from `du` and are approximate. Cargo hard-links each top-level
binary to a file in deps/: the list shows that file's size under both names,
the total counts it once, and its space is freed only once every name is gone.

Examples:
  packaging/prune-target.sh
      List what would be removed from this checkout's target/.
  packaging/prune-target.sh --apply
      Remove it.
  packaging/prune-target.sh --older-than 30 --apply "$CARGO_TARGET_DIR"
      Prune another target directory, keeping a month of release artifacts.

Exit status:
  0  Listed or removed successfully, including when nothing matched.
  1  Refused the target directory, a required command is missing, or a
     removal failed.
  2  Invalid arguments.
EOF
}

while (($# > 0)); do
    case "$1" in
        --apply)
            APPLY=1
            shift
            ;;
        --older-than)
            if (($# < 2)) || [[ ! "$2" =~ ^[0-9]{1,5}$ ]]; then
                printf 'Error: --older-than needs a whole number of days (0-99999)\n' >&2
                usage >&2
                exit 2
            fi
            OLDER_THAN_DAYS=$((10#$2))
            shift 2
            ;;
        --help)
            print_help
            exit 0
            ;;
        -*)
            usage >&2
            exit 2
            ;;
        *)
            if [[ -n "$TARGET_ARG" ]]; then
                usage >&2
                exit 2
            fi
            TARGET_ARG="$1"
            shift
            ;;
    esac
done

for command_name in find du sort rm head tail cut; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        printf 'Missing required command: %s\n' "$command_name" >&2
        exit 1
    fi
done

refuse() {
    printf 'Refusing %s: %s\n' "$TARGET_ARG" "$1" >&2
    exit 1
}

# True when the CACHEDIR.TAG in the given directory is the one cargo writes.
has_cargo_tag() {
    local tag="$1/CACHEDIR.TAG" first_line
    [[ -f "$tag" && ! -L "$tag" ]] || return 1
    first_line="$(head -n 1 -- "$tag")"
    [[ "$first_line" == "$CARGO_TAG_SIGNATURE" ]] || return 1
    [[ "$(head -c 4096 -- "$tag")" == *"created by cargo"* ]]
}

# True when $1 is $2 or a directory containing it.
contains_or_is() {
    [[ "$1" == "/" || "$2" == "$1" || "$2" == "$1"/* ]]
}

if [[ -z "$TARGET_ARG" ]]; then
    TARGET_ARG="$ROOT_DIR/target"
fi
if [[ ! -d "$TARGET_ARG" ]]; then
    refuse 'not a directory'
fi
TARGET="$(cd -- "$TARGET_ARG" && pwd -P)"

if [[ "$TARGET" == "/" ]]; then
    refuse 'it is the filesystem root'
fi

if contains_or_is "$TARGET" "$ROOT_DIR"; then
    refuse "it is or contains the repository root $ROOT_DIR"
fi
if [[ -n "${HOME:-}" && -d "$HOME" ]]; then
    HOME_REAL="$(cd -- "$HOME" && pwd -P)"
    if contains_or_is "$TARGET" "$HOME_REAL"; then
        refuse "it is or contains the home directory $HOME_REAL"
    fi
fi
if ! has_cargo_tag "$TARGET"; then
    refuse "no CACHEDIR.TAG with cargo's signature, so it is not a cargo target directory"
fi

MINUTES=$((OLDER_THAN_DAYS * 1440))
NESTED=()
INCREMENTAL=()
RELEASE=()

# True when a directory is a cargo profile directory such as debug/ or release/.
is_profile() {
    [[ -d "$1/.fingerprint" && ! -L "$1/.fingerprint" ]]
}

# True when a path lies inside one of the nested target directories found.
in_nested() {
    local nested
    for nested in "${NESTED[@]}"; do
        if [[ "$1" == "$nested"/* ]]; then
            return 0
        fi
    done
    return 1
}

# Nested target directories, outermost first; one inside another is covered by
# removing the outer one.
while IFS= read -r -d '' tag; do
    dir="${tag%/CACHEDIR.TAG}"
    if has_cargo_tag "$dir" && ! in_nested "$dir"; then
        NESTED+=("$dir")
    fi
done < <(find "$TARGET" -mindepth 2 -maxdepth 3 -type f -name CACHEDIR.TAG -print0 | sort -z)

# target/<profile>/incremental and target/<triple>/<profile>/incremental.
while IFS= read -r -d '' dir; do
    if is_profile "${dir%/incremental}" && ! in_nested "$dir"; then
        INCREMENTAL+=("$dir")
    fi
done < <(find "$TARGET" -mindepth 2 -maxdepth 3 -type d -name incremental -prune -print0 | sort -z)

# Release artifacts. A candidate is an entry directly inside deps/, build/,
# .fingerprint/ or examples/, or a non-directory at the top of the profile. It
# is kept when it or anything inside it was modified within the cutoff.
collect_release() {
    local profile="$1" rel key first rest
    local -A young=()

    while IFS= read -r -d '' rel; do
        first="${rel%%/*}"
        key="$first"
        if [[ "$rel" == */* ]]; then
            rest="${rel#*/}"
            key="$first/${rest%%/*}"
        fi
        young["$key"]=1
    done < <(find "$profile" -mindepth 1 -mmin "-$MINUTES" -printf '%P\0')

    while IFS= read -r -d '' rel; do
        if [[ -z "${young["$rel"]:-}" ]]; then
            RELEASE+=("$profile/$rel")
        fi
    done < <(
        {
            find "$profile" -mindepth 1 -maxdepth 1 ! -type d \
                ! -name .cargo-lock ! -name '.cargo-*-lock' -printf '%P\0'
            for sub in deps build .fingerprint examples; do
                if [[ -d "$profile/$sub" && ! -L "$profile/$sub" ]]; then
                    find "$profile/$sub" -mindepth 1 -maxdepth 1 -printf "$sub/%P\\0"
                fi
            done
        } | sort -z
    )
}

while IFS= read -r -d '' profile; do
    if is_profile "$profile" && ! in_nested "$profile"; then
        collect_release "$profile"
    fi
done < <(find "$TARGET" -mindepth 1 -maxdepth 2 -type d -name release -print0 | sort -z)

ALL=("${NESTED[@]}" "${INCREMENTAL[@]}" "${RELEASE[@]}")

if ((${#ALL[@]} == 0)); then
    printf 'Target: %s\nNothing to remove.\n' "$TARGET"
    exit 0
fi

# Every path must lie strictly inside the verified target directory.
for path in "${ALL[@]}"; do
    case "$path" in
        "$TARGET"/*) ;;
        *)
            printf 'Internal error: %s is outside %s\n' "$path" "$TARGET" >&2
            exit 1
            ;;
    esac
    case "/$path/" in
        */../* | */./*)
            printf 'Internal error: %s is not a plain path\n' "$path" >&2
            exit 1
            ;;
    esac
done

# Prints each path's size, counting a hard-linked file under every name.
sizes() {
    printf '%s\0' "$@" | du -shl --files0-from=-
}

# Prints the space the paths take together, each hard-linked file once.
total_size() {
    printf '%s\0' "$@" | du -sch --files0-from=- | tail -n 1 | cut -f 1
}

print_group() {
    local title="$1" lines
    shift
    if (($# == 0)); then
        return
    fi
    printf '\n%s (%d):\n' "$title" "$#"
    mapfile -t lines < <(sizes "$@")
    printf '  %s\n' "${lines[@]}"
}

if ((APPLY)); then
    printf 'Target: %s\nRemoving:\n' "$TARGET"
else
    printf 'Target: %s\nWould remove (dry run; nothing is removed without --apply):\n' "$TARGET"
fi
print_group 'Nested target directories' "${NESTED[@]}"
print_group 'Incremental caches' "${INCREMENTAL[@]}"
print_group "Release artifacts not modified in ${OLDER_THAN_DAYS} days" "${RELEASE[@]}"

TOTAL="$(total_size "${ALL[@]}")"
COUNT="${#ALL[@]} paths"
if ((${#ALL[@]} == 1)); then
    COUNT="1 path"
fi
printf '\nTotal: %s, %s\n' "$COUNT" "$TOTAL"

if ((!APPLY)); then
    printf 'Run again with --apply to remove them.\n'
    exit 0
fi

for path in "${ALL[@]}"; do
    rm -rf -- "$path"
done
printf 'Removed %s, about %s.\n' "$COUNT" "$TOTAL"
