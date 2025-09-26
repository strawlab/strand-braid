#!/bin/bash -x
set -o errexit

_packaging/setup-ubuntu-apt-proxy.sh

apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y cpio libudev-dev zlib1g-dev pkg-config curl build-essential git libvpx-dev clang libclang-dev dpkg-dev debhelper zip llvm-dev

ORIG_DIR=`pwd`
echo $ORIG_DIR

# Install nightly Rust. Use specific "known good" version of nightly because
# occasionally breakage happens.
cd /tmp
curl -O --show-error --fail --silent https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init && chmod a+x rustup-init && ./rustup-init -y --default-toolchain nightly-2025-06-20

if [[ -f "$HOME/.cargo/env" ]]; then
    # Put rust on the path (otherwise, it was probably in /usr/bin and on the path anyway).
    . "$HOME/.cargo/env"
fi

rustc --version

# Install trunk (Rust WASM builder and bundler)
if [ "$(uname -m)" = "x86_64" ]; then
    echo "Running on AMD64/x86_64"
    curl --location --remote-name https://github.com/trunk-rs/trunk/releases/download/v0.21.14/trunk-x86_64-unknown-linux-musl.tar.gz
    tar xzf trunk-x86_64-unknown-linux-musl.tar.gz
    mv trunk $CARGO_HOME/bin/
    trunk --version
else
    echo "Running on $(uname -m) architecture"
    cargo install trunk
fi

cd $ORIG_DIR
