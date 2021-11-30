REM Note, this is the wrong way to do things.
REM See https://github.com/rustwasm/wasm-bindgen/pull/1994#issuecomment-608966482
cargo build --target wasm32-unknown-unknown --release --bin freemovr-calibration-webapp || goto :error
wasm-bindgen --target web --no-typescript --out-dir pkg --out-name freemovr-calibration-webapp ../../target/wasm32-unknown-unknown/release/freemovr-calibration-webapp.wasm || goto :error

cargo build --target wasm32-unknown-unknown --release --bin native_worker || goto :error
wasm-bindgen --target no-modules --no-typescript --out-dir pkg --out-name native_worker ../../target/wasm32-unknown-unknown/release/native_worker.wasm || goto :error

copy static\index.html pkg || goto :error
copy static\style.css pkg || goto :error

REM Build OK. Now run with:
REM     microserver --port 8000 --no-spa pkg

goto :EOF

:error
echo Failed with error #%errorlevel%.
exit /b %errorlevel%
