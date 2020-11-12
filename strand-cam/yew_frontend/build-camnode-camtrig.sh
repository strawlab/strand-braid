#!/bin/bash -x
set -o errexit

cargo --frozen web build --release --features "with_camtrig"

mkdir -p dist
cp -a target/wasm32-unknown-unknown/release/strand-cam-frontend-yew.js target/wasm32-unknown-unknown/release/strand-cam-frontend-yew.wasm dist/
cd dist
ln -sf ../static/index.html
ln -sf ../static/style.css
ln -sf ../static/strand-camera-no-text.png
