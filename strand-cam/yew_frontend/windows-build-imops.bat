wasm-pack build --target web -- --no-default-features

mkdir pkg
copy static\index.html pkg
copy static\style.css pkg
copy static\strand-camera-no-text.png pkg
