#!/bin/bash
set -o errexit

# Prerequisite: flydra2-mainbrain/frontend/pkg is built.

RUSTFLAGS="$RUSTFLAGS -C target-cpu=sandybridge -C codegen-units=1" NUM_JOBS=2 cargo build --no-default-features --features "bundle_files backtrace" --release
