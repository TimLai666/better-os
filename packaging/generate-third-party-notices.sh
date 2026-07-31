#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_FILE="$ROOT_DIR/docs/third-party-licenses.md"
CHECK_ONLY=0

usage() {
    printf 'Usage: %s [--check] [--output FILE]\n' "$0"
}

while (($# > 0)); do
    case "$1" in
        --check)
            CHECK_ONLY=1
            shift
            ;;
        --output)
            if (($# < 2)); then
                usage >&2
                exit 2
            fi
            OUTPUT_FILE="$2"
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

for command_name in cargo jq sha256sum mktemp cmp; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        printf 'Missing required command: %s\n' "$command_name" >&2
        exit 1
    fi
done

METADATA_FILE="$(mktemp)"
GENERATED_FILE="$(mktemp)"
trap 'rm -f "$METADATA_FILE" "$GENERATED_FILE"' EXIT

cargo metadata --format-version 1 --locked >"$METADATA_FILE"
LOCK_SHA256="$(sha256sum "$ROOT_DIR/Cargo.lock" | awk '{print $1}')"
MISSING_METADATA_COUNT="$(jq '[.packages[] | select(.license == null and .license_file == null)] | length' "$METADATA_FILE")"

{
    printf '%s\n' \
        '# Third-Party License Notices' \
        '' \
        'This inventory is generated from the locked Rust dependency graph with' \
        '`cargo metadata --format-version 1 --locked`. It is distributed with each' \
        'Better OS Debian package under `/usr/share/doc/<package>/`.' \
        '' \
        'The inventory records the package license metadata and source reference.' \
        'It does not replace the license text supplied by each upstream project.' \
        '' \
        '## Review summary' \
        '' \
        '- Root project license: `GPL-3.0-or-later`.' \
        "- Resolved Cargo packages: $(jq '.packages | length' "$METADATA_FILE")." \
        "- Packages with an SPDX license expression: $(jq '[.packages[] | select(.license != null)] | length' "$METADATA_FILE")." \
        "- Packages with only a license-file field: $(jq '[.packages[] | select(.license == null and .license_file != null)] | length' "$METADATA_FILE")." \
        "- Packages without package-level license metadata: $(jq '[.packages[] | select(.license == null and .license_file == null)] | length' "$METADATA_FILE")." \
        "- \`Cargo.lock\` SHA-256: \`$LOCK_SHA256\`." \
        '' \
        "The $MISSING_METADATA_COUNT package(s) without package-level license metadata are" \
        'listed separately below. Their pinned upstream checkout contains both' \
        '`LICENSE-GPL` and `LICENSE-APACHE`; file-level upstream markings remain the' \
        'source of truth for those packages.' \
        '' \
        '## License expression counts' \
        '' \
        '| License expression | Package records |' \
        '| --- | ---: |'
    jq -r '
        [.packages[] | select(.license != null) | .license]
        | group_by(.)
        | map({license: .[0], count: length})
        | sort_by([-.count, .license])[]
        | "| `\(.license | gsub("\\|"; "\\\\|"))` | \(.count) |"
    ' "$METADATA_FILE"
    printf '%s\n' \
        '' \
        '## Review focus' \
        '' \
        'The following records contain copyleft or additional notice-sensitive' \
        'license expressions. Their upstream expressions are preserved verbatim;' \
        'Better OS does not relicense or silently select a different expression.' \
        '' \
        '| Package | Version | License expression | Source |' \
        '| --- | --- | --- | --- |'
    jq -r '
        def escape: gsub("\\|"; "\\\\|") | gsub("[\\r\\n]"; " ");
        def source_reference:
            if .source == null then "workspace"
            elif (.source | startswith("registry+")) then
                "[crates.io](https://crates.io/crates/\(.name)/\(.version))"
            else
                "`\(.source | escape)`"
            end;
        .packages
        | map(select(.license != null and (.license | test("GPL|LGPL|MPL|NCSA|bzip2"; "i"))))
        | sort_by([(.name | ascii_downcase), .version])[]
        | "| `\(.name | escape)` | `\(.version | escape)` | `\(.license | escape)` | \(source_reference) |"
    ' "$METADATA_FILE"
    printf '%s\n' \
        '' \
        '## Package inventory' \
        '' \
        '| Package | Version | License metadata | Source |' \
        '| --- | --- | --- | --- |'
    jq -r '
        def escape: gsub("\\|"; "\\\\|") | gsub("[\\r\\n]"; " ");
        def source_reference:
            if .source == null then "workspace"
            elif (.source | startswith("registry+")) then
                "[crates.io](https://crates.io/crates/\(.name)/\(.version))"
            else
                "`\(.source | escape)`"
            end;
        .packages
        | sort_by([(.name | ascii_downcase), .version])[]
        | "| `\(.name | escape)` | `\(.version | escape)` | `\((if .license != null then .license elif .license_file != null then "license-file: \(.license_file)" else "missing package metadata" end) | escape)` | \(source_reference) |"
    ' "$METADATA_FILE"
    printf '%s\n' \
        '' \
        '## Packages requiring metadata review' \
        '' \
        '| Package | Version | Source | Review note |' \
        '| --- | --- | --- | --- |'
    jq -r '
        def escape: gsub("\\|"; "\\\\|") | gsub("[\\r\\n]"; " ");
        def source_reference:
            if .source == null then "workspace"
            else "`\(.source | escape)`"
            end;
        .packages
        | map(select(.license == null and .license_file == null))
        | sort_by([(.name | ascii_downcase), .version])[]
        | "| `\(.name | escape)` | `\(.version | escape)` | \(source_reference) | Pinned upstream Zed workspace package has no package-level license metadata; retain the upstream GPL/APACHE notices and review file-level markings. |"
    ' "$METADATA_FILE"
} >"$GENERATED_FILE"

if ((CHECK_ONLY)); then
    if ! cmp -s "$GENERATED_FILE" "$OUTPUT_FILE"; then
        printf 'Third-party license notice inventory is stale: %s\n' "$OUTPUT_FILE" >&2
        exit 1
    fi
    printf 'Third-party license notice inventory is current: %s\n' "$OUTPUT_FILE"
else
    mkdir -p "$(dirname -- "$OUTPUT_FILE")"
    mv "$GENERATED_FILE" "$OUTPUT_FILE"
    printf 'Generated third-party license notice inventory: %s\n' "$OUTPUT_FILE"
fi
