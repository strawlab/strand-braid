REM cargo web build --release
REM rd /s /q dist
REM mkdir dist
REM copy target\wasm32-unknown-unknown\release\strand-cam-frontend-yew.js dist
REM copy target\wasm32-unknown-unknown\release\strand-cam-frontend-yew.wasm dist
REM copy static\index.html dist
REM copy static\style.css dist
REM copy static\strand-camera-no-text.png dist

wasm-pack build --target web

mkdir pkg
copy static\index.html pkg
copy static\style.css pkg
copy static\strand-camera-no-text.png pkg
