#!/bin/bash
set -o errexit

# Prerequisite: ../yew_frontend/pkg is built. Do this by "build-imops.sh" in yew_frontend.

export VIMBAC_LIBDIR="/opt/Vimba_6_0/VimbaC/DynamicLib/x86_64bit"
export PKG_CONFIG_PATH=/opt/opencv-4.5.5-static/lib/pkgconfig:/opt/libvpx/libvpx-1.8.0/lib/pkgconfig

cargo +nightly build --features backtrace,ci2-vimba/backtrace,imops/simd --release
