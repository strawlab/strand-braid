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

# Note: this is not a good general-purpose way to install wasm-pack, because it
# does not install wasm-bindgen. Instead, use the installer at
# https://rustwasm.github.io/wasm-pack/installer/. We use this approach here
# because it is faster for our CI builds.
mkdir -p $CARGO_HOME/bin && curl --show-error --fail --silent https://internal-static.strawlab.org/software/wasm-pack/wasm-pack-0.8.1-amd64.exe > $CARGO_HOME/bin/wasm-pack
chmod a+x $CARGO_HOME/bin/wasm-pack
export PATH="$PATH:$CARGO_HOME/bin"
wasm-pack --version

# Install grass (SASS proceessor)
cargo install grass

# Install trunk (Rust WASM builder and bundler)
cargo install trunk

# TODO: include firmware bundled
rustc --version

cd $ORIG_DIR
