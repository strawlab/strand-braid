#!/bin/bash -x
set -o errexit

# See https://github.com/rust-lang/cargo/issues/1970

rm -f ../pyci2/*.so
rm -f target_linux/debug/*.so
CARGO_TARGET_DIR="target_linux" cargo build --no-default-features --features pylon
cp target_linux/debug/lib_pyci2.so ../pyci2/_pyci2.so
