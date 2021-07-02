Show creation time with:

    ffmpeg -i <intput_ts> -f null out.null

View timestamps with:

    mkvinfo --all <intput_ts>

or

    ffmpeg -debug_ts -re -copyts -i <intput_ts> -f null out.null

or

    ffprobe -show_packets -i <intput_ts>

# Build on Windows

```
# Download https://github.com/ShiftMediaProject/libvpx/releases/download/v1.9.0/libvpx_v1.9.0_msvc16.zip
# and unzip into %HomeDrive%%HomePath%\libvpx_v1.9.0_msvc16

set VPX_VERSION=1.9.0
set VPX_STATIC=1
set VPX_LIB_DIR=%HomeDrive%%HomePath%\libvpx_v1.9.0_msvc16\lib\x64
set VPX_INCLUDE_DIR=%HomeDrive%%HomePath%\libvpx_v1.9.0_msvc16\include
SET VPX_NO_PKG_CONFIG=1
```
