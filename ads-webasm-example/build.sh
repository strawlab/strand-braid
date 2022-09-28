#!/bin/bash -x
set -o errexit

wasm-pack build --target web

cp static/index.html pkg/index.html
grass -I ../ads-webasm/scss static/ads-webasm-example.scss > pkg/style.css
