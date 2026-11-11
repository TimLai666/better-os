#!/usr/bin/env bash
# Drives packaging/prune-target.sh against synthetic cargo target directories.
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$ROOT_DIR/packaging/prune-target.sh"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf -- "$WORK_DIR"' EXIT

FAILURES=0
OUTPUT=""
STATUS=0

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    FAILURES=$((FAILURES + 1))
}

pass() {
    printf 'ok: %s\n' "$1"
}

# Runs the script with the given arguments, capturing combined output and the
# exit status without tripping `set -e`.
run() {
    STATUS=0
    OUTPUT="$("$@" 2>&1)" || STATUS=$?
}

expect_status() {
    local expected="$1" label="$2"
    if [[ "$STATUS" == "$expected" ]]; then
        pass "$label (exit $STATUS)"
    else
        fail "$label: expected exit $expected, got $STATUS; output: $OUTPUT"
    fi
}

expect_output() {
    local needle="$1" label="$2"
    if [[ "$OUTPUT" == *"$needle"* ]]; then
        pass "$label"
    else
        fail "$label: output lacks '$needle'; output: $OUTPUT"
    fi
}

expect_gone() {
    local path
    for path in "$@"; do
        if [[ -e "$path" || -L "$path" ]]; then
            fail "expected removed: $path"
        else
            pass "removed: ${path#"$WORK_DIR"/}"
        fi
    done
}

expect_kept() {
    local path
    for path in "$@"; do
        if [[ -e "$path" || -L "$path" ]]; then
            pass "kept: ${path#"$WORK_DIR"/}"
        else
            fail "expected kept: $path"
        fi
    done
}

cargo_tag() {
    mkdir -p "$1"
    printf '%s\n' \
        'Signature: 8a477f597d28d172789f06886806bc55' \
        '# This file is a cache directory tag created by cargo.' \
        '# For information about cache directory tags see https://bford.info/cachedir/' \
        >"$1/CACHEDIR.TAG"
}

# A file of a given size in KiB, so the reported totals are not all zero.
blob() {
    mkdir -p "$(dirname -- "$1")"
    head -c "$(($2 * 1024))" /dev/zero >"$1"
}

# Marks a path and everything under it as last modified 30 days ago.
age() {
    find "$1" -exec touch -h -d '30 days ago' {} +
}

snapshot() {
    find "$1" -printf '%P %y %s\n' | sort
}

# --- The synthetic target -------------------------------------------------
T="$WORK_DIR/target"
cargo_tag "$T"

# The debug build in use. Even its old files stay: they are what keeps the next
# `cargo build` from compiling everything again.
blob "$T/debug/deps/libserde-aaa.rlib" 8
blob "$T/debug/.fingerprint/serde-aaa/lib-serde" 1
blob "$T/debug/build/ring-bbb/out/libring.a" 8
blob "$T/debug/better-launcher" 4
touch "$T/debug/.cargo-lock"
age "$T/debug"
blob "$T/debug/incremental/launcher-xyz/s-abc/query-cache.bin" 16

# Release artifacts: some older than the cutoff, some fresh.
blob "$T/release/deps/libold-111.rlib" 8
blob "$T/release/.fingerprint/old-111/lib-old" 1
blob "$T/release/build/old-444/out/gen.rs" 2
blob "$T/release/build/mixed-333/out/stale.o" 2
blob "$T/release/better-app" 4
# cargo hard-links each top-level binary to its copy in deps/.
ln "$T/release/better-app" "$T/release/deps/better_app-999"
touch "$T/release/.cargo-lock" "$T/release/.cargo-build-lock" "$T/release/.cargo-artifact-lock"
blob "$WORK_DIR/outside-sentinel" 1
ln -s "$WORK_DIR/outside-sentinel" "$T/release/deps/link-to-outside"
age "$T/release"
age "$WORK_DIR/outside-sentinel"
blob "$T/release/build/mixed-333/out/fresh.o" 2
blob "$T/release/deps/libnew-222.rlib" 8
blob "$T/release/.fingerprint/new-222/lib-new" 1
blob "$T/release/better-new" 4
blob "$T/release/incremental/app-rel/s-1/dep-graph.bin" 4

# A cross-compilation profile under a target triple.
blob "$T/x86_64-unknown-linux-gnu/release/deps/libcross-555.rlib" 4
mkdir -p "$T/x86_64-unknown-linux-gnu/release/.fingerprint" "$T/x86_64-unknown-linux-gnu/debug/.fingerprint"
age "$T/x86_64-unknown-linux-gnu"
blob "$T/x86_64-unknown-linux-gnu/debug/incremental/cross-1/s-1/x.bin" 4

# Other toolchains' target directories nested inside this one.
cargo_tag "$T/msrv-check"
blob "$T/msrv-check/debug/deps/libserde-ccc.rlib" 8
cargo_tag "$T/clippy-198"
blob "$T/clippy-198/debug/incremental/a/b.bin" 4

# Not cargo's: a cache tag with a different body, and cargo's own scratch dirs.
mkdir -p "$T/not-cargo"
printf 'Signature: 8a477f597d28d172789f06886806bc55\n# Created by some other tool.\n' \
    >"$T/not-cargo/CACHEDIR.TAG"
blob "$T/not-cargo/data.bin" 1
blob "$T/tmp/test-scratch" 1
# A test's scratch files under names cargo uses, but not in a profile directory.
blob "$T/tmp/release/old-scratch" 1
blob "$T/tmp/incremental/scratch" 1
age "$T/tmp"
blob "$T/doc/index.html" 1
age "$T/doc"

REMOVED=(
    "$T/debug/incremental"
    "$T/release/incremental"
    "$T/x86_64-unknown-linux-gnu/debug/incremental"
    "$T/msrv-check"
    "$T/clippy-198"
    "$T/release/deps/libold-111.rlib"
    "$T/release/deps/link-to-outside"
    "$T/release/.fingerprint/old-111"
    "$T/release/build/old-444"
    "$T/release/better-app"
    "$T/release/deps/better_app-999"
    "$T/x86_64-unknown-linux-gnu/release/deps/libcross-555.rlib"
)
KEPT=(
    "$T/CACHEDIR.TAG"
    "$T/debug/deps/libserde-aaa.rlib"
    "$T/debug/.fingerprint/serde-aaa/lib-serde"
    "$T/debug/build/ring-bbb/out/libring.a"
    "$T/debug/better-launcher"
    "$T/debug/.cargo-lock"
    "$T/release/.cargo-lock"
    "$T/release/.cargo-build-lock"
    "$T/release/.cargo-artifact-lock"
    "$T/release/build/mixed-333/out/stale.o"
    "$T/release/build/mixed-333/out/fresh.o"
    "$T/release/deps/libnew-222.rlib"
    "$T/release/.fingerprint/new-222/lib-new"
    "$T/release/better-new"
    "$T/not-cargo/data.bin"
    "$T/tmp/test-scratch"
    "$T/tmp/release/old-scratch"
    "$T/tmp/incremental/scratch"
    "$T/doc/index.html"
    "$WORK_DIR/outside-sentinel"
)

# --- Help and usage ---------------------------------------------------------
run bash "$SCRIPT" --help
expect_status 0 '--help'
for word in --apply --older-than --help TARGET_DIR 'Exit status' Examples 'default: 7'; do
    expect_output "$word" "--help documents $word"
done

run bash "$SCRIPT" --bogus "$T"
expect_status 2 'unknown option is a usage error'
run bash "$SCRIPT" --older-than
expect_status 2 '--older-than without a value is a usage error'
run bash "$SCRIPT" --older-than 3x "$T"
expect_status 2 '--older-than with a non-number is a usage error'
run bash "$SCRIPT" "$T" "$T"
expect_status 2 'two target directories is a usage error'

# --- Dry run ------------------------------------------------------------------
before="$(snapshot "$WORK_DIR")"
run bash "$SCRIPT" "$T"
expect_status 0 'dry run'
after="$(snapshot "$WORK_DIR")"
if [[ "$before" == "$after" ]]; then
    pass 'dry run changes nothing on disk'
else
    fail 'dry run changed the tree'
    diff <(printf '%s\n' "$before") <(printf '%s\n' "$after") >&2 || true
fi
for path in "${REMOVED[@]}"; do
    expect_output "$path" "dry run lists ${path#"$T"/}"
done
for path in "${KEPT[@]}"; do
    if [[ "$OUTPUT" == *"$path"$'\n'* || "$OUTPUT" == *"$path" ]]; then
        fail "dry run lists a path it must keep: $path"
    fi
done
expect_output 'Total: 12 paths' 'dry run reports the path count'
expect_output '--apply' 'dry run says how to apply'

# --- Apply --------------------------------------------------------------------
run bash "$SCRIPT" --apply "$T"
expect_status 0 '--apply'
expect_output 'Removed 12 paths' '--apply reports what it removed'
expect_gone "${REMOVED[@]}"
expect_kept "${KEPT[@]}"

run bash "$SCRIPT" "$T"
expect_status 0 'second dry run'
expect_output 'Nothing to remove' 'a pruned tree has nothing left to remove'

# --- --older-than 0 takes every release artifact ------------------------------
run bash "$SCRIPT" --older-than 0 --apply "$T"
expect_status 0 '--older-than 0 --apply'
expect_gone "$T/release/deps/libnew-222.rlib" "$T/release/build/mixed-333" "$T/release/better-new"
expect_kept "$T/release/.cargo-lock" "$T/debug/deps/libserde-aaa.rlib" "$T/debug/better-launcher"

# --- Refusals ---------------------------------------------------------------------
run bash "$SCRIPT" "$WORK_DIR/missing"
expect_status 1 'a missing directory is refused'
expect_output 'not a directory' 'a missing directory is named as such'

mkdir -p "$WORK_DIR/untagged"
run bash "$SCRIPT" "$WORK_DIR/untagged"
expect_status 1 'a directory without CACHEDIR.TAG is refused'
expect_output "cargo's signature" 'the missing tag is named as the reason'

mkdir -p "$WORK_DIR/other-tag"
printf 'Signature: 8a477f597d28d172789f06886806bc55\n# Created by some other tool.\n' \
    >"$WORK_DIR/other-tag/CACHEDIR.TAG"
run bash "$SCRIPT" "$WORK_DIR/other-tag"
expect_status 1 'a CACHEDIR.TAG without cargo in it is refused'
expect_output "cargo's signature" 'the foreign tag is named as the reason'

run bash "$SCRIPT" /
expect_status 1 '/ is refused'
expect_output 'filesystem root' '/ is named as the filesystem root'

# A home directory and its parent, each carrying cargo's tag, so only the
# dangerous-path check can refuse them.
cargo_tag "$WORK_DIR/homes"
cargo_tag "$WORK_DIR/homes/user"
run env HOME="$WORK_DIR/homes/user" bash "$SCRIPT" "$WORK_DIR/homes/user"
expect_status 1 'the home directory is refused'
expect_output 'home directory' 'the home directory is named as the reason'
run env HOME="$WORK_DIR/homes/user" bash "$SCRIPT" "$WORK_DIR/homes"
expect_status 1 'a parent of the home directory is refused'
expect_output 'home directory' 'the parent of home is refused for home'

# A copy of the script in a fake repository, to reach the repo-root check and
# the default target directory.
REPO="$WORK_DIR/repo"
mkdir -p "$REPO/packaging"
cp "$SCRIPT" "$REPO/packaging/prune-target.sh"
cargo_tag "$REPO"
run bash "$REPO/packaging/prune-target.sh" "$REPO"
expect_status 1 'the repository root is refused'
expect_output 'repository root' 'the repository root is named as the reason'
run bash "$REPO/packaging/prune-target.sh" "$REPO/packaging/.."
expect_status 1 'the repository root is refused through ..'

run bash "$REPO/packaging/prune-target.sh"
expect_status 1 'a repository without target/ is refused'
cargo_tag "$REPO/target"
blob "$REPO/target/debug/incremental/a/b.bin" 1
mkdir -p "$REPO/target/debug/.fingerprint"
run bash "$REPO/packaging/prune-target.sh"
expect_status 0 'the default target is the repository target/'
expect_output "$REPO/target/debug/incremental" 'the default target is listed'

# A symlinked target is followed to the real one and judged there.
ln -s "$REPO" "$WORK_DIR/repo-link"
run bash "$REPO/packaging/prune-target.sh" "$WORK_DIR/repo-link"
expect_status 1 'a symlink to the repository root is refused'
expect_output 'repository root' 'the symlink is judged by where it points'

if ((FAILURES > 0)); then
    printf '%d check(s) failed\n' "$FAILURES" >&2
    exit 1
fi
printf 'All prune-target checks passed\n'
