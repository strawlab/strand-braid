REM Prerequisite: ../yew_frontend/dist is built. Do this by "build.bat" in yew_frontend.

set PYLON_VERSION=6

cargo build --no-default-features --features strand-cam/bundle_files,imops/simd --release || goto :error

goto :EOF

:error
echo Failed with error #%errorlevel%.
exit /b %errorlevel%
