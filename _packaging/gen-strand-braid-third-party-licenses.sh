#!/bin/bash
# Generate per-crate THIRD-PARTY-LICENSES files for every crate whose
# binary ships in the strand-braid Ubuntu bundle / Debian package.
#
# Usage:
#   gen-strand-braid-third-party-licenses.sh [<output-dir>]
#   gen-strand-braid-third-party-licenses.sh --list
#
# <output-dir> defaults to ./licenses . With --list, print the crate paths
# (one per line) and exit without generating anything.
#
# This list is the single source of truth for which crates' licenses ship in
# the strand-braid bundle. It must cover every binary installed by
# _packaging/strand-braid/debian/strand-braid.install; CI enforces that via
# _packaging/check-strand-braid-third-party-licenses.sh.
set -euo pipefail

# Crate paths (relative to the repo root) whose transitive dependencies'
# licenses ship in the strand-braid bundle.
CRATES=(
  braid
  braid/braid-offline
  braid/braid-process-video
  braid/braid-run
  braidz/braidz-parser/braidz-cli
  braidz/braidz-rerun/braidz-export-rrd
  braidz/braidz-rerun/rerun-braidz-viewer
  braidz/flytrax-csv-to-braidz
  geometry/braid-april-cal/braid-april-cal-cli
  geometry/braid-mvg/mvg-util
  geometry/braidz-mcsc
  geometry/mcsc-native/gocal
  im-proc/flydra-feature-detector
  media-utils/fmf/fmf-cli
  media-utils/mp4-bframe-doctor
  media-utils/show-timestamps
  media-utils/strand-convert
  strand-cam
)

if [[ "${1:-}" == "--list" ]]; then
  printf '%s\n' "${CRATES[@]}"
  exit 0
fi

OUTPUT_DIR="${1:-licenses}"
HERE="$(cd "$(dirname "$0")" && pwd)"

exec "$HERE/gen-third-party-licenses.sh" "$OUTPUT_DIR" "${CRATES[@]}"
