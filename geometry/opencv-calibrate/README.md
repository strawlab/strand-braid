# opencv-calibrate

A wrapper of OpenCV `cvCalibrateCamera2` and `findChessboardCorners` functions.

## Building OpenCV on macOS and linux for static linking

On macOS, I found that I had to compile and install OpenCV statically. To do so
(installing into `$HOME/devroot`), I did the following:

    # expand opencv source 4.11.0 and enter into its dir
    mkdir build
    cd build
    cmake -DCMAKE_INSTALL_PREFIX=$HOME/devroot -DWITH_FFMPEG=OFF -DWITH_TIFF=OFF -DBUILD_SHARED_LIBS=OFF -DBUILD_opencv_highgui=OFF -DWITH_GSTREAMER=OFF -DBUILD_DOCS=OFF -DBUILD_opencv_python=OFF -DWITH_1394=OFF -DWITH_CUDA=OFF -DWITH_CUFFT=OFF -DWITH_JASPER=OFF -DWITH_LIBV4L=OFF -DWITH_OPENCL=OFF -DWITH_EIGEN=OFF -DWITH_JPEG=OFF -DWITH_PNG=OFF -DWITH_OPENEXR=OFF -DBUILD_ZLIB=OFF -DBUILD_TESTS=OFF -DBUILD_PERF_TESTS=OFF -DBUILD_FAT_JAVA_LIB=OFF -DWITH_GTK=OFF -DBUILD_opencv_contrib=OFF -DBUILD_opencv_gpu=OFF -DBUILD_opencv_stitching=OFF -DBUILD_opencv_nonfree=OFF -DBUILD_opencv_stitching=OFF -DBUILD_opencv_nonfree=OFF -DBUILD_opencv_legacy=OFF -DBUILD_opencv_superres=OFF -DBUILD_ITT=OFF -DBUILD_IPP_IW=OFF -DWITH_IPP=OFF -DWITH_ITT=OFF -DOPENCV_GENERATE_PKGCONFIG=YES -DWITH_CAROTENE=OFF -DWITH_LAPACK=OFF ..
    rm -rf $HOME/devroot/lib/libopencv_* # remove any pre-existing libs (see below)
    make install

Note that a prior dynamic build takes precedence in linking. Thus the line above
to remove any prior opencv install `rm -rf $HOME/devroot/lib/libopencv_*`.

Now, build any subsequent dependent package with env var `PKG_CONFIG_PATH`
appropriately. For the above, it would be:

    PKG_CONFIG_PATH=$HOME/devroot/lib/pkgconfig cargo build

## Dynamic linking on ubuntu or debian

On ubuntu or debian, the `libopencv-dev` package installs the `opencv4.pc` file
which will be detected automatically. Thus:

    sudo apt-get install -y libopencv-dev
    cargo build

## Building on Windows

**Note** This section should be updated for OpenCV 4.

```
certUtil -hashfile C:\Users\drand\Downloads\opencv-4.5.5-vc14_vc15.exe SHA256
SHA256 hash of C:\Users\drand\Downloads\opencv-4.5.5-vc14_vc15.exe:
cac31973cd1c59bfe9dc926acbde815553d23662ea355e0414b5e50d8f8aa5a8
```

Here is what I did in a `.bat` file to do dynamic linking. This worked:

    echo on

    REM tested with opencv 3.2
    set OPENCV_VERSION=455
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

As tested on Ubuntu 20.04, `apt install libopencv-dev` will install the required
libraries and pkg-config file.

## License

This crate is Copyright (C) 2020 Andrew Straw <strawman@astraw.com>.

Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
http://opensource.org/licenses/MIT>, at your option. This file may not be
copied, modified, or distributed except according to those terms.
