REM Download opencv-3.2.0-vc14.exe from https://sourceforge.net/projects/opencvlibrary/files/opencv-win/3.2.0/opencv-3.2.0-vc14.exe/download
REM then expand it in your Downloads directory.

set OPENCV_VERSION=320
set OPENCV_LIB_DIR=%HomeDrive%%HomePath%\Downloads\opencv\build\x64\vc14\lib
set OPENCV_INCLUDE_DIR=%HomeDrive%%HomePath%\Downloads\opencv\build\include

REM Now build the binary

cargo build --release

copy %HomeDrive%%HomePath%\Downloads\opencv\build\x64\vc14\bin\opencv_world320.dll ..\..\target\release\
