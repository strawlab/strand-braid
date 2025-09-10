#!/bin/bash
set -o errexit

# Prerequisite: ../yew_frontend/dist is built. Do this by "build.sh" in yew_frontend.

export PKG_CONFIG_PATH=/opt/opencv-4.5.5-static/lib/pkgconfig

cargo +nightly build --features imops/simd --release
