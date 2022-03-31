#!/bin/bash -x
set -o errexit

wasm-pack build --target web -- --features with_led_box,flydratrax,checkercal

cd pkg
ln -sf ../static/index.html
ln -sf ../static/style.css
ln -sf ../static/strand-camera-no-text.png
