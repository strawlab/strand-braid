#!/bin/bash -x
set -euo pipefail

wasm-pack build --target web --dev --features ads-webasm/obj

cp static/index.html pkg/index.html
grass -I ../ads-webasm/scss static/ads-webasm-example.scss pkg/style.css

echo "Build OK. Now run with:\n"
echo "    microserver --port 8000 --no-spa pkg"
