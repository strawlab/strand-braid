wasm-pack build --target web --dev --features ads-webasm/obj

mkdir pkg
copy static\index.html pkg
grass -I ../scss static/ads-webasm-example.scss pkg/style.css
