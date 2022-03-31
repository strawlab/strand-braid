wasm-pack build --target web -- --features with_led_box,flydratrax

mkdir pkg
copy static\index.html pkg
copy static\style.css pkg
copy static\strand-camera-no-text.png pkg
