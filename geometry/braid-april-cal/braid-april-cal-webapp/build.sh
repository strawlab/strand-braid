#!/bin/bash
set -o errexit

# Install wasm-pack from here https://rustwasm.github.io/wasm-pack/installer/

# This will build the source and place results into a new `pkg` dir
wasm-pack build --target web

cp static/index.html pkg/index.html
grass -I ../../../ads-webasm/scss/ static/braid-april-cal-webapp.scss pkg/style.css

echo Build OK. Now run with:
echo     microserver --port 8000 --no-spa pkg
echo and visit http://localhost:8000/
