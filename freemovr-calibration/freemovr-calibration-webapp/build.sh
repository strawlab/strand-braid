#!/bin/bash -x
set -o errexit

# Note, this is the wrong way to do things.
# See https://github.com/rustwasm/wasm-bindgen/pull/1994#issuecomment-608966482
cargo build --target wasm32-unknown-unknown --release --bin freemovr-calibration-webapp
wasm-bindgen --target web --no-typescript --out-dir pkg --out-name freemovr-calibration-webapp ../../target/wasm32-unknown-unknown/release/freemovr-calibration-webapp.wasm

cargo build --target wasm32-unknown-unknown --release --bin native_worker
wasm-bindgen --target no-modules --no-typescript --out-dir pkg --out-name native_worker ../../target/wasm32-unknown-unknown/release/native_worker.wasm

mkdir -p pkg
cp static/index.html pkg
grass -I ../../ads-webasm/scss/ static/freemovr-calibration-webapp.scss pkg/style.css

echo Build OK. Now run with:
echo     microserver --port 8000 --no-spa pkg
