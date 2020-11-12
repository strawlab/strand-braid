#!/bin/bash -x
set -o errexit

# See https://github.com/rust-lang/cargo/issues/1970

rm -f ../pyci2/*.so
rm -f target_mac/debug/*.dylib
#DC1394_LIBDIR="$HOME/devroot/lib" CARGO_TARGET_DIR="target_mac" cargo build -- -C link-arg=-undefined -C link-arg=dynamic_lookup
DC1394_LIBDIR="$HOME/devroot/lib" CARGO_TARGET_DIR="target_mac" cargo rustc --lib -- -C link-arg=-undefined -C link-arg=dynamic_lookup
cp target_mac/debug/lib_pyci2.dylib ../pyci2/_pyci2.so
