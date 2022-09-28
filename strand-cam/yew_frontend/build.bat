REM Install wasm-pack from here https://rustwasm.github.io/wasm-pack/installer/
wasm-pack build --target web

REM Install grass with: cargo install grass
grass -I ../../../ads-webasm/scss scss/strand-cam-frontend.scss > pkg/style.css
