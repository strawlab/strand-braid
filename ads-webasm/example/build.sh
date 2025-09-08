#!/bin/bash -x
set -euo pipefail

trunk build --features ads-webasm/obj

echo "Build OK. Now run with:\n"
echo "    microserver --port 8080 --no-spa dist"
