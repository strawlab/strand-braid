#!/bin/bash -x
set -o errexit

wasm-pack build --release --target web -- --features checkercal
