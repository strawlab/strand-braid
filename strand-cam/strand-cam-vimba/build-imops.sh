#!/bin/bash
set -o errexit

# Prerequisite: ../yew_frontend/pkg is built. Do this by "build-imops.sh" in yew_frontend.

export VIMBAC_LIBDIR="/opt/vimba/Vimba_5_1/VimbaC/DynamicLib/x86_64bit"
export PKG_CONFIG_PATH=/opt/opencv-4.5.5-static/lib/pkgconfig:/opt/libvpx/libvpx-1.8.0/lib/pkgconfig

cargo +nightly build --no-default-features --features bundle_files,backend_vimba,backtrace,ci2-vimba/backtrace --release
