REM Note, this is the wrong way to do things.
REM See https://github.com/rustwasm/wasm-bindgen/pull/1994#issuecomment-608966482
cargo build --target wasm32-unknown-unknown --release --bin main || goto :error
wasm-bindgen --target web --no-typescript --out-dir pkg --out-name main ../target/wasm32-unknown-unknown/release/main.wasm || goto :error

copy static\index.html pkg\ || goto :error
grass -I ../../../ads-webasm/scss/ static/braid-april-cal-webapp.scss pkg/style.css

REM Build OK. Now run with:
REM     microserver --port 8000 --no-spa pkg
REM and visit http://localhost:8000/

goto :EOF

:error
echo Failed with error #%errorlevel%.
exit /b %errorlevel%
