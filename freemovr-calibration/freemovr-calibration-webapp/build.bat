REM Note, this is the wrong way to do things.
REM See https://github.com/rustwasm/wasm-bindgen/pull/1994#issuecomment-608966482
cargo build --target wasm32-unknown-unknown --release --bin main
wasm-bindgen --target web --no-typescript --out-dir pkg --out-name main target/wasm32-unknown-unknown/release/main.wasm

cargo build --target wasm32-unknown-unknown --release --bin native_worker
wasm-bindgen --target no-modules --no-typescript --out-dir pkg --out-name native_worker target/wasm32-unknown-unknown/release/native_worker.wasm

mkdir pkg
copy static\index.html pkg
copy static\style.css pkg
