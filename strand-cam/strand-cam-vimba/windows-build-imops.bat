REM Prerequisite: ../yew_frontend/dist is built. Do this by "build.bat" in yew_frontend.

cargo build --no-default-features --features strand-cam/bundle_files,imops/simd --release
