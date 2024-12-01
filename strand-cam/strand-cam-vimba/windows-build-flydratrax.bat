REM Prerequisite: ../yew_frontend/pkg is built. Do this by "build-flydratrax.bat" in yew_frontend.

REM Download opencv-3.2.0-vc14.exe from https://sourceforge.net/projects/opencvlibrary/files/opencv-win/3.2.0/opencv-3.2.0-vc14.exe/download
REM then expand it in your Downloads directory.

set OPENCV_VERSION=320
set OPENCV_LIB_DIR=%HomeDrive%%HomePath%\Downloads\opencv\build\x64\vc14\lib
set OPENCV_INCLUDE_DIR=%HomeDrive%%HomePath%\Downloads\opencv\build\include

@REM Set IPPROOT environment variable: run this script in a shell opened with:
@REM cmd /k "c:\Program Files (x86)\IntelSWTools\compilers_and_libraries_2019\windows\ipp\bin\ippvars.bat" intel64

cargo build --no-default-features --features "strand-cam/bundle_files strand-cam/flydratrax strand-cam/imtrack-dark-circle ipp-sys/2019 imops/simd strand-cam/use_ipp" --release
