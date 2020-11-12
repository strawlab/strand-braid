#!/bin/bash -x
set -o errexit

cd elm_frontend
elm-package install -y
npm install
elm-app build # requires https://github.com/halfzebra/create-elm-app
cd ..

cargo build --release --no-default-features --features "serve_static flycap2"
