REM Prerequisite: yew_frontend/pkg is built. Do this by "windows-build-camtrig.bat" in yew_frontend.

REM Download opencv-3.2.0-vc14.exe from https://sourceforge.net/projects/opencvlibrary/files/opencv-win/3.2.0/opencv-3.2.0-vc14.exe/download
REM then expand it in your Downloads directory.

set OPENCV_VERSION=320
set OPENCV_LIB_DIR=%HomeDrive%%HomePath%\Downloads\opencv\build\x64\vc14\lib
set OPENCV_INCLUDE_DIR=%HomeDrive%%HomePath%\Downloads\opencv\build\include

@REM Set IPPROOT environment variable: run this script in a shell opened with:
@REM cmd /k "c:\Program Files (x86)\IntelSWTools\compilers_and_libraries_2019\windows\ipp\bin\ippvars.bat" intel64

REM Now build the binary

set PYLON_VERSION=6

@REM Download https://github.com/ShiftMediaProject/libvpx/releases/download/v1.10.0/libvpx_v1.10.0_msvc16.zip
@REM and unzip into %HomeDrive%%HomePath%\libvpx_v1.10.0_msvc16
set VPX_VERSION=1.10.0
set VPX_STATIC=1
set VPX_LIB_DIR=%HomeDrive%%HomePath%\libvpx_v1.10.0_msvc16\lib\x64
set VPX_INCLUDE_DIR=%HomeDrive%%HomePath%\libvpx_v1.10.0_msvc16\include
SET VPX_NO_PKG_CONFIG=1
cargo build --no-default-features --features strand-cam/bundle_files,strand-cam/flydratrax,strand-cam/with_led_box,ipp-sys/2019,strand-cam/imtrack-absdiff,strand-cam/image_tracker,strand-cam/checkercal,backtrace --release

copy %HomeDrive%%HomePath%\Downloads\opencv\build\x64\vc14\bin\opencv_world320.dll ..\..\target\release\
