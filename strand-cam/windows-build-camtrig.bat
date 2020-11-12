REM Prerequisite: yew_frontend/pkg is built. Do this by "windows-build-camtrig.bat" in yew_frontend.

set OPENCV_VERSION=320
set OPENCV_LIB_DIR=%HomeDrive%%HomePath%\Downloads\opencv\build\x64\vc14\lib
set OPENCV_INCLUDE_DIR=%HomeDrive%%HomePath%\Downloads\opencv\build\include

REM Now build the binary

set PYLON_VERSION=6

@REM Download https://github.com/ShiftMediaProject/libvpx/releases/download/v1.9.0/libvpx_v1.9.0_msvc16.zip
@REM and unzip into %HomeDrive%%HomePath%\libvpx_v1.9.0_msvc16
set VPX_VERSION=1.9.0
set VPX_STATIC=1
set VPX_LIB_DIR=%HomeDrive%%HomePath%\libvpx_v1.9.0_msvc16\lib\x64
set VPX_INCLUDE_DIR=%HomeDrive%%HomePath%\libvpx_v1.9.0_msvc16\include
SET VPX_NO_PKG_CONFIG=1
cargo build --no-default-features --features bundle_files,backend_pyloncxx,flydratrax,camtrig,ipp-sys/2019,imtrack-absdiff,image_tracker,cfg-pt-detect-src-prefs,checkercal --release
