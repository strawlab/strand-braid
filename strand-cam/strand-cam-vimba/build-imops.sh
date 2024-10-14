#!/bin/bash
set -o errexit

# Prerequisite: ../yew_frontend/pkg is built. Do this by "build-imops.sh" in yew_frontend.

export PKG_CONFIG_PATH=/opt/opencv-4.5.5-static/lib/pkgconfig

cargo +nightly build --features backtrace,ci2-vimba/backtrace,imops/simd --release
