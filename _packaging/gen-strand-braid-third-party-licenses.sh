#!/bin/bash
# Generate per-crate THIRD-PARTY-LICENSES files for every crate whose
# binary ships in the strand-braid Ubuntu bundle / Debian package.
#
# Usage:
#   gen-strand-braid-third-party-licenses.sh [<output-dir>]
#
# <output-dir> defaults to ./licenses .
#
# Keep this list in sync with _packaging/strand-braid/debian/strand-braid.install.
set -euo pipefail

OUTPUT_DIR="${1:-licenses}"
HERE="$(cd "$(dirname "$0")" && pwd)"

exec "$HERE/gen-third-party-licenses.sh" "$OUTPUT_DIR" \
  braid \
  braid/braid-offline \
  braid/braid-process-video \
  braid/braid-run \
  braidz/braidz-parser/braidz-cli \
  braidz/braidz-rerun/braidz-export-rrd \
  braidz/braidz-rerun/rerun-braidz-viewer \
  braidz/flytrax-csv-to-braidz \
  geometry/braid-april-cal/braid-april-cal-cli \
  geometry/braidz-mcsc \
  geometry/mcsc-native/gocal \
  geometry/opencv-calibrate \
  im-proc/flydra-feature-detector \
  media-utils/fmf/fmf-cli \
  media-utils/show-timestamps \
  media-utils/strand-convert \
  strand-cam/strand-cam-pylon \
  strand-cam/strand-cam-vimba
