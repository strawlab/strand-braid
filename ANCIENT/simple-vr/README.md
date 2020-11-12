# simple-vr

## assumptions

Observer is always at z=0 with x,y given by the configuration setting
`tracker.pixels_to_meters_matrix3`. Display width and height is given by config
setting `vr_display.width_meters` and `vr_display.height_meters`.

The origin in world coordinates (i.e. `0,0,0`) is defined to be the observer
position at which `tracker.pixels_to_meters_matrix3` computes a position of
`0,0`. The center of the VR display is at world coordinate
`(0,0,vr_display.distance_to_screen_meters)`. So, therefore the matrix
`tracker.pixels_to_meters_matrix3` should ensure that the (`0,0`) position is
directly under then VR display center.

## building and running

Compiling and running on Windows:

    # install Visual Studio Community Edition 2015
    # install Point Gray FlyCapture 2 SDK
    # download https://static.rust-lang.org/rustup/dist/i686-pc-windows-gnu/rustup-init.exe
    # download ippicv from https://sourceforge.net/projects/opencvlibrary/files/3rdparty/ippicv/ and unpack it.

    .\rustup-init.exe --default-toolchain stable-x86_64-pc-windows-msvc
    # Change the directory below to the where you unpacked ippicv.
    set IPPICV_DIR=C:\Users\astraw\Documents\other-peoples-src\ippicv\ippicv_windows_20141027\ippicv_win

    cargo run --no-default-features --features flycap --target x86_64-pc-windows-msvc --release --bin simple-vr -- data\config.json

Alternatively, you can build it:

    cargo build --no-default-features --features flycap --target x86_64-pc-windows-msvc --release

and then run it:

    target\release\simple-vr.exe data\config.json
