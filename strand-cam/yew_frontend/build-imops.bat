REM Install wasm-pack from here https://rustwasm.github.io/wasm-pack/installer/

REM This will build the source and place results into a new `pkg` dir
wasm-pack build --target web -- --no-default-features

REM Install grass with: cargo install grass
grass -I ..\..\ads-webasm\scss scss\strand-cam-frontend.scss pkg\style.css

copy static\index.html pkg\index.html
copy static\strand-camera-no-text.png pkg\strand-camera-no-text.png
