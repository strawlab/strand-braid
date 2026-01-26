#!/bin/bash
set -o errexit

# Install trunk as described here https://trunkrs.dev/#install
trunk build --release

echo "Build OK. Now run with:\n"
echo "    trunk serve --open"
