#!/bin/bash
set -o errexit

# Prerequisite: ../yew_frontend/pkg is built. Do this by "build.sh" in yew_frontend.

PKG_CONFIG_PATH=/opt/libvpx/libvpx-1.8.0/lib/pkgconfig \
RUSTFLAGS="$RUSTFLAGS -C target-cpu=sandybridge -C codegen-units=1" \
NUM_JOBS=2 \
cargo build --features "strand-cam/imtrack-absdiff strand-cam/bundle_files backtrace" --release
