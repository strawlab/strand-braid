#!/bin/bash -x
set -o errexit

trunk build --release

echo Build OK. Now run with:
echo     microserver --port 8000 --no-spa dist
