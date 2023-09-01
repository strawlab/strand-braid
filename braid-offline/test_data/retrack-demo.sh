#!/bin/bash -x
set -o errexit

BASE=../_submodules/flydra/flydra_analysis/flydra_analysis/a2/sample_datafile-v0.4.28
RETRACKED=`mktemp -d -t retrackedXXXXXXX`

NGCU=../strand-braid-user

# Convert original flydra file to .csv
python $NGCU/scripts/export_h5_to_csv.py "$BASE.h5"
# Retrack .csv files
cargo build --release
BUILD_DIR=../target/release
$BUILD_DIR/braid-offline-retrack -d "$BASE" -o "$RETRACKED.braidz"

# Convert to pytables .h5 format
rm -f "$RETRACKED.braidz.h5"
PATH="$BUILD_DIR:$PATH" python $NGCU/scripts/convert_braidz_to_flydra_h5.py "$RETRACKED.braidz"

# flydra_analysis_plot_timeseries_2d_3d "$BASE".h5 -k "$BASE".h5 --disable-kalman-smoothing
flydra_analysis_plot_timeseries_2d_3d "$RETRACKED".braidz.h5 -k "$RETRACKED".braidz.h5 --disable-kalman-smoothing

rm "$RETRACKED.braidz.h5"
rm "$RETRACKED.unconverted.zip"
rm -rf "$RETRACKED"
