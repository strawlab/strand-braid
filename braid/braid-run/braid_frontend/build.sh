#!/bin/bash
set -o errexit

trunk build --release

echo "Build OK. Now run with:\n"
echo "    trunk serve --open"
