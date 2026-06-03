#!/bin/bash
set -o errexit

cargo build --no-default-features --features strand-cam/bundle_files --release

# To run, set the DYLD_LIBRARY_PATH environment variable
#
#    export DYLD_LIBRARY_PATH=/Library/Frameworks/pylon.framework/Versions/A/Libraries
