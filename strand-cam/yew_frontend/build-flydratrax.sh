#!/bin/bash -x
set -o errexit

wasm-pack build --target web -- --features flydratrax,checkercal
