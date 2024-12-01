REM Download opencv-4.5.5-vc14_vc15.exe from https://github.com/opencv/opencv/releases/download/4.5.5/opencv-4.5.5-vc14_vc15.exe
REM then expand it in your Downloads directory.

set OPENCV_VERSION=455
set OPENCV_LIB_DIR=%HomeDrive%%HomePath%\Downloads\opencv\build\x64\vc15\lib
set OPENCV_INCLUDE_DIR=%HomeDrive%%HomePath%\Downloads\opencv\build\include

@REM Set IPPROOT environment variable: run this script in a shell opened with:
@REM cmd /k "c:\Program Files (x86)\IntelSWTools\compilers_and_libraries_2019\windows\ipp\bin\ippvars.bat" intel64

cargo build --no-default-features --features "strand-cam/bundle_files strand-cam/flydra_feat_detect strand-cam/imtrack-absdiff ipp-sys/2019 strand-cam/checkercal imops/simd strand-cam/use_ipp" --release
