#!/bin/bash
set -o errexit

trunk build --release

echo Build OK. Now run with:
echo     microserver --port 8000 --no-spa dist
echo and visit http://localhost:8000/
