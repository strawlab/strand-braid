#!/bin/bash
set -o errexit

# Prerequisite: ../yew_frontend/pkg is built. Do this by "build-imops.sh" in yew_frontend.

export VIMBAC_LIBDIR="/opt/vimba/Vimba_5_1/VimbaC/DynamicLib/x86_64bit"
export PKG_CONFIG_PATH=/opt/opencv-3.2-static/lib/pkgconfig:/opt/libvpx/libvpx-1.8.0/lib/pkgconfig

source /opt/intel/bin/compilervars.sh -arch intel64 -platform linux && \
OPENCV_STATIC=1 \
PKG_CONFIG_PATH=/opt/opencv-3.2-static/lib/pkgconfig:/opt/libvpx/libvpx-1.8.0/lib/pkgconfig \
IPP_STATIC=1 \
RUSTFLAGS="$RUSTFLAGS -C target-cpu=sandybridge -C codegen-units=1 -C link-args=-Wl,-rpath,/opt/vimba/Vimba_5_1/VimbaC/DynamicLib/x86_64bit" \
NUM_JOBS=2 \
cargo build --features "strand-cam/bundle_files strand-cam/posix_sched_fifo ipp-sys/2019 strand-cam/checkercal strand-cam/fiducial backtrace imops/simd" --release
