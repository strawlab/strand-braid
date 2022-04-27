REM Download opencv-4.5.5-vc14_vc15.exe from https://github.com/opencv/opencv/releases/download/4.5.5/opencv-4.5.5-vc14_vc15.exe
REM then expand it in your Downloads directory.

set OPENCV_VERSION=455
set OPENCV_LIB_DIR=%HomeDrive%%HomePath%\Downloads\opencv\build\x64\vc15\lib
set OPENCV_INCLUDE_DIR=%HomeDrive%%HomePath%\Downloads\opencv\build\include

copy %OPENCV_LIB_DIR%\..\bin\opencv_world455.dll .
cargo test --release --features backtrace
