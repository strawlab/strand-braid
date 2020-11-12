wasm-pack build --target web -- --features flydratrax

mkdir pkg
copy static\index.html pkg
copy static\style.css pkg
copy static\strand-camera-no-text.png pkg
