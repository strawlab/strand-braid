#!/bin/bash
set -o errexit

# Install wasm-pack from here https://rustwasm.github.io/wasm-pack/installer/

# This will build the source and place results into a new `pkg` dir
wasm-pack build --target web

# Install grass with: cargo install grass
grass -I ../../../ads-webasm/scss scss/braid_frontend.scss pkg/style.css

cp static/braid-logo-no-text.png pkg/braid-logo-no-text.png
cp static/index.html pkg/index.html

# above built everything, let's now run it locally
# (install with `cargo install microserver`)
echo "Build OK. Now run with:\n"
echo "    microserver --port 8000 --no-spa pkg"
