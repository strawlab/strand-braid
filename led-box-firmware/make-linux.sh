#!/bin/bash -x
set -o errexit

CARGO_TARGET_DIR=target_linux cargo build --release

arm-none-eabi-objcopy -O binary target_linux/thumbv7em-none-eabihf/release/led-box-firmware target_linux/thumbv7em-none-eabihf/release/led-box-firmware.bin

