#!/bin/bash
set -o errexit

# Note, this is the wrong way to do things.
# See https://github.com/rustwasm/wasm-bindgen/pull/1994#issuecomment-608966482
cargo build --target wasm32-unknown-unknown --release --bin main
wasm-bindgen --target web --no-typescript --out-dir pkg --out-name main ../target/wasm32-unknown-unknown/release/main.wasm

cp static/index.html pkg/
cp static/style.css pkg/

echo Build OK. Now run with:
echo     microserver --port 8000 --no-spa pkg
echo and visit http://localhost:8000/
