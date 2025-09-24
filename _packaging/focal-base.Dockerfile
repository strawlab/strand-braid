# Ensure that you build from a clean git checkout, otherwise you might have lots
# of extraneous files in the Docker image.
#
# Build with:
#    docker build --platform linux/amd64 -f _packaging/focal-base.Dockerfile --progress=plain .
# Tag with:
#    docker tag <the_sha1_hash_of_the_image> gitlab.strawlab.org:4567/straw/rust-cam/focal-base:0.0.3
# Push with:
#    docker push gitlab.strawlab.org:4567/straw/rust-cam/focal-base:0.0.3
FROM ubuntu:focal

# Although this is redundant with _packaging/setup-ubuntu-base.sh, we do it here to put it in the docker cache.
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y cpio libudev-dev zlib1g-dev pkg-config curl build-essential git libvpx-dev clang libclang-dev dpkg-dev debhelper zip llvm-dev

ENV CARGO_HOME=/usr

WORKDIR /usr/src/strand-braid
COPY . .

RUN _packaging/setup-ubuntu-base.sh && \
    rustup toolchain install stable && \
    rustup target add wasm32-unknown-unknown
