#!/bin/bash
set -o errexit

cd gen-nvenc-bindings
cargo run -- ../src/ffi.rs
