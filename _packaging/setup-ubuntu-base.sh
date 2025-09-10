#!/bin/bash -x
set -o errexit

_packaging/setup-ubuntu-apt-proxy.sh

apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y cpio libudev-dev zlib1g-dev pkg-config curl build-essential git libvpx-dev

ORIG_DIR=`pwd`
echo $ORIG_DIR

# Install nightly Rust. Use specific "known good" version of nightly because
# occasionally breakage happens.
cd /tmp
curl -O --show-error --fail --silent https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init && chmod a+x rustup-init && ./rustup-init -y --default-toolchain nightly-2025-06-20

export PATH="$PATH:$CARGO_HOME/bin"

# Install trunk (Rust WASM builder and bundler)
cargo install trunk

# TODO: include firmware bundled
rustc --version

cd $ORIG_DIR
