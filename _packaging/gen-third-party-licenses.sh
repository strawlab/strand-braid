#!/bin/bash
# Generate THIRD-PARTY-LICENSES files for one or more crates.
#
# Usage:
#   gen-third-party-licenses.sh <output-dir> <crate-path> [<crate-path>...]
#
# For each <crate-path> (relative to the repo root), writes
#   <output-dir>/THIRD-PARTY-LICENSES.<basename-of-crate-path>.yml
# containing the YAML-formatted license texts of that crate's transitive
# Cargo dependencies, with MIT preferred where multiple licenses are
# available.
#
# Requires `cargo-bundle-licenses` to be on $PATH. Install with:
#   cargo install cargo-bundle-licenses --locked
#
# `cargo-bundle-licenses` resolves dependencies for whichever crate's
# Cargo.toml is in the current working directory (it has no
# `--manifest-path` flag), so this script `cd`s into each crate.
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <output-dir> <crate-path> [<crate-path>...]" >&2
  exit 1
fi

OUTPUT_DIR="$1"
shift

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$OUTPUT_DIR"
# Resolve to an absolute path so `--output` works after we `cd` into a crate.
OUTPUT_DIR="$(cd "$OUTPUT_DIR" && pwd)"

for CRATE_PATH in "$@"; do
  CRATE_NAME="$(basename "$CRATE_PATH")"
  OUT="$OUTPUT_DIR/THIRD-PARTY-LICENSES.$CRATE_NAME.yml"
  echo "Generating $OUT for $CRATE_PATH ..."
  (
    cd "$REPO_ROOT/$CRATE_PATH"
    cargo bundle-licenses \
      --format yaml \
      --prefer MIT \
      --output "$OUT"
  )
done
