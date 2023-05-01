REM Prerequisite: ../yew_frontend/pkg is built. Do this by "build-plain.bat" in yew_frontend.

cargo build --no-default-features --features "strand-cam/bundle_files strand-cam/imtrack-absdiff backtrace" --release
