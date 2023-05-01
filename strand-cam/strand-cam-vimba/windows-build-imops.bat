REM Prerequisite: ../yew_frontend/pkg is built. Do this by "build-imops.bat" in yew_frontend.

cargo build --no-default-features --features strand-cam/bundle_files,backtrace,imops/simd --release
