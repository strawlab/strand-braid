#!/bin/bash
set -o errexit

# Prerequisite: ../yew_frontend/dist is built. Do this by "build.sh" in yew_frontend.

export PKG_CONFIG_PATH=/opt/opencv-4.5.5-static/lib/pkgconfig

source /opt/intel/bin/compilervars.sh -arch intel64 -platform linux && \
OPENCV_STATIC=1 \
PKG_CONFIG_PATH=/opt/opencv-4.5.5-static/lib/pkgconfig \
IPP_STATIC=1 \
RUSTFLAGS="$RUSTFLAGS -C target-cpu=sandybridge -C codegen-units=1" \
NUM_JOBS=2 \
cargo build --features "strand-cam/flydra_feat_detect strand-cam/use_ipp strand-cam/imtrack-absdiff strand-cam/bundle_files ipp-sys/2019 strand-cam/checkercal strand-cam/fiducial imops/simd" --release
