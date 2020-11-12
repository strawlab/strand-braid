FROM ubuntu:12.04

RUN apt-get update -y && apt-get dist-upgrade -y

RUN apt-get install build-essential curl pkg-config -y
RUN curl https://sh.rustup.rs -sSf | sh -s -- --default-toolchain nightly-2017-01-06 -y

RUN apt-get install libdc1394-22-dev -y

VOLUME /src

# cd /src/rust-cam/fview2 && /root/.cargo/bin/cargo build --release
