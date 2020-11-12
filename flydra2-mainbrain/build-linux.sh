#!/bin/bash
set -o errexit

# This is used on linux to build the mainbrain.

source $HOME/.cargo/env

# source $HOME/ros/flydra-kinetic/devel/setup.bash

cd yew_frontend
./build.sh
cd ..

export CI_PROJECT_DIR=`pwd`/..
export ROSRUST_MSG_PATH=$CI_PROJECT_DIR/_submodules:$CI_PROJECT_DIR/_submodules/ros_comm_msgs:$CI_PROJECT_DIR/_submodules/common_msgs:$CI_PROJECT_DIR/image-tracker
cargo build --release --features "ros"
