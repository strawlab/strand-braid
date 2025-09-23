# Run with:
#    docker build --platform linux/amd64 -f _packaging/focal-base.Dockerfile .
# Tag with:
#    docker tag <the_sha1_hash_of_the_image> gitlab.strawlab.org:4567/straw/rust-cam/focal-base:0.0.1
# Push with:
#    docker push gitlab.strawlab.org:4567/straw/rust-cam/focal-base:0.0.1
FROM ubuntu:focal

ENV CARGO_HOME=/usr

WORKDIR /usr/src/strand-braid
COPY . .

RUN _packaging/setup-ubuntu-base.sh
