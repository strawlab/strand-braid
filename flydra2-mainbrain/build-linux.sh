#!/bin/bash
set -o errexit

# This is used on linux to build the mainbrain.

source $HOME/.cargo/env

# source $HOME/ros/flydra-kinetic/devel/setup.bash

cd yew_frontend
./build.sh
cd ..

export CI_PROJECT_DIR=`pwd`/..
cargo build --release --features "ros"
