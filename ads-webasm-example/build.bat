wasm-pack build --target web --dev --features ads-webasm/obj

mkdir pkg
copy static\index.html pkg
grass -I ../ads-webasm/scss static/ads-webasm-example.scss pkg/style.css
