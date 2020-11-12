#!/bin/bash -x
set -o errexit

sudo docker run \
     -it \
     --rm \
     --name fview2-flycap-precise \
     -v `pwd`/../../../..:/src \
     -v /tmp/precise-cargo-git:/root/.cargo/git \
     -v /tmp/precise-cargo-registry:/root/.cargo/registry \
     fview2-flycap-precise
