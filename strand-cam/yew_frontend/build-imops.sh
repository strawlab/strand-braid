wasm-pack build --target web -- --no-default-features

mkdir -p pkg
cd pkg
ln -sf ../static/index.html
ln -sf ../static/style.css
ln -sf ../static/strand-camera-no-text.png
