REM Install wasm-pack from here https://rustwasm.github.io/wasm-pack/installer/
wasm-pack build --target web

mkdir pkg
copy static\* pkg
