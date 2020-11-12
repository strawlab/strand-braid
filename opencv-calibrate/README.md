# opencv-calibrate

A wrapper around OpenCV's `cvCalibrateCamera2` function.

## Building OpenCV for static linking.

On macOS, I found that I had to compile and install OpenCV statically. To do so
(installing into `$HOME/devroot`), I did the following:

    # expand opencv source 2.4.11 and enter into its dir
    mkdir build
    cd build
    cmake -DCMAKE_INSTALL_PREFIX=$HOME/devroot -DWITH_FFMPEG=OFF -DWITH_TIFF=OFF -DBUILD_SHARED_LIBS=OFF -DBUILD_opencv_highgui=OFF -DWITH_GSTREAMER=OFF -DBUILD_DOCS=OFF -DBUILD_opencv_python=OFF -DWITH_1394=OFF -DWITH_CUDA=OFF -DWITH_CUFFT=OFF -DWITH_JASPER=OFF -DWITH_LIBV4L=OFF -DWITH_OPENCL=OFF -DWITH_EIGEN=OFF -DWITH_JPEG=OFF -DWITH_PNG=OFF -DWITH_OPENEXR=OFF -DBUILD_ZLIB=OFF -DBUILD_TESTS=OFF -DBUILD_PERF_TESTS=OFF -DBUILD_FAT_JAVA_LIB=OFF -DWITH_GTK=OFF DBUILD_ZLIB=ON -DBUILD_opencv_contrib=OFF -DBUILD_opencv_gpu=OFF -DBUILD_opencv_stitching=OFF -DBUILD_opencv_nonfree=OFF -DBUILD_opencv_stitching=OFF -DBUILD_opencv_nonfree=OFF -DBUILD_opencv_legacy=OFF -DBUILD_opencv_superres=OFF ..
    rm -rf $HOME/devroot/lib/libopencv_* # see below
    make install

Note that a prior dynamic build takes precedence in linking. Thus the line above
to remove any prior opencv install `rm -rf $HOME/devroot/lib/libopencv_*`.

Now, build any subsequent dependent package with env var `PKG_CONFIG_PATH`
appropriately. For the above, it would be:

    PKG_CONFIG_PATH=$HOME/devroot/lib/pkgconfig cargo build

On linux, I have packaged `opencv-3.2-static.tar.gz` which installs to
`/opt/opencv-3.2-static/`. Thus:

    OPENCV_STATIC=1 PKG_CONFIG_PATH=/opt/opencv-3.2-static/lib/pkgconfig cargo build

## Building on Windows

```
certUtil -hashfile C:\Users\drand\Downloads\opencv-3.2.0-vc14.exe SHA256
SHA256 hash of C:\Users\drand\Downloads\opencv-3.2.0-vc14.exe:
3e2b73fe6d0f84f8947a3a5d8776f93ec4d3eb7a0a3e32c13cd1cddfda85f786
```

Here is what I did in a `.bat` file to do dynamic linking. This worked:

    echo on

    REM tested with opencv 3.2
    set OPENCV_VERSION=320
    set OPENCV_LIB_DIR=C:\Users\astraw\Downloads\opencv\build\x64\vc14\lib
    set OPENCV_INCLUDE_DIR=C:\Users\astraw\Downloads\opencv\build\include
    cargo build

Here with powershell:

```
$Env:OPENCV_VERSION="320"
$Env:OPENCV_LIB_DIR="C:\Users\astraw\Downloads\opencv\build\x64\vc14\lib"
$Env:OPENCV_INCLUDE_DIR="C:\Users\drand\Downloads\opencv\build\include"
```

Here is what I did in a `.bat` file to do static linking. This did not work and I stopped
working further on it:

    echo on

    REM tested with opencv 2.4.13.6
    set OPENCV_STATIC=1
    set OPENCV_VERSION=2413
    set OPENCV_LIB_DIR=C:\Users\astraw\Downloads\opencv\build\x64\vc14\staticlib
    set OPENCV_INCLUDE_DIR=C:\Users\astraw\Downloads\opencv\build\include
    cargo build

## Installation note for Ubuntu

As tested on Ubuntu 16.04, `apt install libopencv-dev` will install the required
libraries and pkg-config file.
