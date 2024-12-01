#!/bin/bash
set -o errexit

# It seems the Basler drivers are not compiled for aarch64-apple-darwin, so build for x86:
cargo build --no-default-features --features strand-cam/bundle_files,imops/simd --release --target x86_64-apple-darwin

# To run, set the DYLD_LIBRARY_PATH environment variable
#
#    export DYLD_LIBRARY_PATH=/Library/Frameworks/pylon.framework/Versions/A/Libraries
