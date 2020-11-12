REM Download opencv-3.2.0-vc14.exe from https://sourceforge.net/projects/opencvlibrary/files/opencv-win/3.2.0/opencv-3.2.0-vc14.exe/download
REM then expand it in your Downloads directory.

set OPENCV_VERSION=320
set OPENCV_LIB_DIR=%HomeDrive%%HomePath%\Downloads\opencv\build\x64\vc14\lib
set OPENCV_INCLUDE_DIR=%HomeDrive%%HomePath%\Downloads\opencv\build\include
set OPENCV_STATIC=1

REM For now, I am getting a link error in which zlib.lib is not found. I guess
REM it is reference by the OpenCV static library but does not seem to be packaged
REM with it. I have stopped working on this issue for now and thus this static
REM link test fails on Windows. (The DLL linking is working.)

cargo test -- --nocapture
