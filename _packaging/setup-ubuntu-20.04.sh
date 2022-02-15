#!/bin/bash -x
set -o errexit

apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y cpio libudev-dev libapriltag-dev libssl-dev zlib1g-dev pkg-config curl build-essential git

ORIG_DIR=`pwd`
echo $ORIG_DIR

# Install IPP
mkdir -p /tmp/download-ipp
cd /tmp/download-ipp
curl -O --silent https://internal-static.strawlab.org/software/ipp/l_ipp_2019.3.199.tgz
curl -O --silent https://internal-static.strawlab.org/software/ipp/install-ipp-2019.sh
chmod a+x install-ipp-2019.sh
/tmp/download-ipp/install-ipp-2019.sh
cd /
rm -rf /tmp/download-ipp

# Install nightly Rust
cd /tmp
curl -O --silent https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init && chmod a+x rustup-init && ./rustup-init -y --default-toolchain nightly

# Note: this is not a good general-purpose way to install wasm-pack, because it does not install wasm-bindgen.
# Instead, use the installer at https://rustwasm.github.io/wasm-pack/installer/.
mkdir -p $CARGO_HOME/bin && curl --show-error --fail --silent https://internal-static.strawlab.org/software/wasm-pack/wasm-pack-0.8.1-amd64.exe > $CARGO_HOME/bin/wasm-pack
chmod a+x $CARGO_HOME/bin/wasm-pack
export PATH="$PATH:$CARGO_HOME/bin"
wasm-pack --version

# TODO: include firmware bundled
rustc --version
curl --show-error --fail --silent https://internal-static.strawlab.org/software/libvpx/libvpx-opt-static_1.8.0-0ads1_amd64.deb > /tmp/libvpx-opt-static_1.8.0-0ads1_amd64.deb
echo "b47f14efcb5cb35e7a17300094e2e5c7daba8bbdc6610a0463f5933cda61a1de /tmp/libvpx-opt-static_1.8.0-0ads1_amd64.deb" | sha256sum -c
apt-get install /tmp/libvpx-opt-static_1.8.0-0ads1_amd64.deb

# Download and install pylon 6
# When updating this, also update the file ubuntu-2004-installer-zip-readme.txt and the Pylon version specified in strand-braid/debian/control
curl --show-error --fail --silent https://internal-static.strawlab.org/software/pylon/pylon_6.2.0.21487-deb0_amd64.deb > /tmp/pylon_6.2.0.21487-deb0_amd64.deb
echo "6acaf99a7331fde2b82217b15642a4e1ae96022bb13c7a91ed1a929ae664e391 /tmp/pylon_6.2.0.21487-deb0_amd64.deb" | sha256sum -c
apt-get install /tmp/pylon_6.2.0.21487-deb0_amd64.deb

curl --show-error --fail --silent https://internal-static.strawlab.org/software/opencv/opencv-3.2-static.tar.gz > /tmp/opencv-3.2-static.tar.gz
echo "0316517e848ab3193b8d3ce2d7275602466dbd396e465b7aae5a9c7f342290d4  /tmp/opencv-3.2-static.tar.gz" | sha256sum -c
tar xzf /tmp/opencv-3.2-static.tar.gz -C /

cd $ORIG_DIR
