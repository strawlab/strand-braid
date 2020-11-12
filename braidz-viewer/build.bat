REM Install wasm-pack from here https://rustwasm.github.io/wasm-pack/installer/
wasm-pack build --target web

mkdir deploy
copy pkg\* deploy
copy static\* deploy

REM Build OK. Now run with:
REM     microserver --port 8000 --no-spa deploy
