REM Prerequisite: ../yew_frontend/dist is built. Do this by "build.bat" in yew_frontend.

set PYLON_VERSION=6

cargo build --no-default-features --features "strand-cam/bundle_files strand-cam/imtrack-absdiff" --release
