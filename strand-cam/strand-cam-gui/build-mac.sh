#!/bin/bash -x
set -o errexit

cargo build --no-default-features --features "strand-cam/serve_files strand-cam/flydratrax" --release
