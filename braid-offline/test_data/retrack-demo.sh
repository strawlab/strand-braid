#!/bin/bash
set -o errexit

BASE=../_submodules/flydra/flydra_analysis/flydra_analysis/a2/sample_datafile-v0.4.28
RETRACKED=/tmp/kalmanized

NGCU=../strand-braid-user

# Convert original flydra file to .csv
python $NGCU/scripts/export_h5_to_csv.py "$BASE.h5"
# Retrack .csv files
cargo build
RUST_LOG=flydra2=trace ../target/debug/offline-retrack -d "$BASE" -o "$RETRACKED"

# Convert to pytables .h5 format
rm -f "$RETRACKED.braidz.h5"
PATH="../target/debug:$PATH" python $NGCU/scripts/convert_kalmanized_csv_to_flydra_h5.py "$RETRACKED.braidz"

# flydra_analysis_plot_timeseries_2d_3d "$BASE".h5 -k "$BASE".h5 --disable-kalman-smoothing
flydra_analysis_plot_timeseries_2d_3d "$RETRACKED".braidz.h5 -k "$RETRACKED".braidz.h5 --disable-kalman-smoothing
