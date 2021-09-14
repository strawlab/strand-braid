REM Note, this is the wrong way to do things.
REM See https://github.com/rustwasm/wasm-bindgen/pull/1994#issuecomment-608966482
cargo build --target wasm32-unknown-unknown --release --bin main
wasm-bindgen --target web --no-typescript --out-dir braid-april-cal-webapp --out-name main ../target/wasm32-unknown-unknown/release/main.wasm

cargo build --target wasm32-unknown-unknown --release --bin native_worker
wasm-bindgen --target no-modules --no-typescript --out-dir braid-april-cal-webapp --out-name native_worker ../target/wasm32-unknown-unknown/release/native_worker.wasm

copy static\index.html braid-april-cal-webapp\
copy static\style.css braid-april-cal-webapp\

REM Build OK. Now run with:
REM     microserver --port 8000 --no-spa .
REM and visit http://localhost:8000/braid-april-cal-webapp/
