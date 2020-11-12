#!/bin/bash
set -o errexit

DEPLOY_DIR=deploy

# Install wasm-pack from here https://rustwasm.github.io/wasm-pack/installer/
wasm-pack build --target web
mkdir -p $DEPLOY_DIR
rm -rf $DEPLOY_DIR/*
cp pkg/* $DEPLOY_DIR/
cp static/* $DEPLOY_DIR/

# above built everything, let's now run it locally
# (install with `cargo install microserver`)
echo "Build OK. Now run with:\n"
echo "    microserver --port 8000 --no-spa deploy"
