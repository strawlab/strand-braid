#!/bin/bash -x
set -o errexit

cargo --frozen web build --release

mkdir -p dist
cp -a target/wasm32-unknown-unknown/release/rt-image-viewer-frontend-yew.js target/wasm32-unknown-unknown/release/rt-image-viewer-frontend-yew.wasm dist/
cd dist
ln -sf ../static/index.html
