#!/bin/bash
set -o errexit

# Install trunk as described here https://trunk-rs.github.io/trunk/guide/getting-started/installation.html
trunk build --release

echo "Build OK. Now run with:\n"
echo "    trunk serve --open"
