wasm-pack build --target web -- --no-default-features || goto :error

mkdir pkg
copy static\index.html pkg
copy static\style.css pkg
copy static\strand-camera-no-text.png pkg

goto :EOF

:error
echo Failed with error #%errorlevel%.
exit /b %errorlevel%
