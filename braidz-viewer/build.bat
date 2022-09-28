REM Install wasm-pack from here https://rustwasm.github.io/wasm-pack/installer/
wasm-pack build --target web

mkdir deploy
copy pkg\* deploy
copy static\* deploy
grass -I ../ads-webasm/scss/ scss/braidz-viewer.scss > deploy/style.css

REM Build OK. Now run with:
REM     microserver --port 8000 --no-spa deploy
