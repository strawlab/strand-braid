REM Install wasm-pack from here https://rustwasm.github.io/wasm-pack/installer/

REM This will build the source and place results into a new `pkg` dir
wasm-pack build --target web

REM Install grass with: cargo install grass
grass -I ../../../ads-webasm/scss scss/braid_frontend.scss pkg/style.css

copy static\braid-logo-no-text.png pkg\braid-logo-no-text.png
copy static\index.html pkg\index.html
