#!/bin/bash
set -o errexit

# Prerequisite: ../yew_frontend/dist is built. Do this by "build.sh" in yew_frontend.

export PKG_CONFIG_PATH=/opt/opencv-4.5.5-static/lib/pkgconfig

OPENCV_STATIC=1 \
PKG_CONFIG_PATH=/opt/opencv-4.5.5-static/lib/pkgconfig \
RUSTFLAGS="$RUSTFLAGS -C codegen-units=1" \
NUM_JOBS=2 \
cargo build --features "strand-cam/flydra_feat_detect strand-cam/imtrack-absdiff strand-cam/bundle_files strand-cam/checkercal strand-cam/fiducial" --release
