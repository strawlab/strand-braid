#!/bin/bash
set -o errexit

# Install wasm-pack from here https://rustwasm.github.io/wasm-pack/installer/
wasm-pack build --target web

# above built everything, let's now run it locally
# (install with `cargo install microserver`)
echo "Build OK. Now run with:\n"
echo "    microserver --port 8000 --no-spa pkg"
