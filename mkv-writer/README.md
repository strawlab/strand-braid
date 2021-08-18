# mkv-writer

Save video to a Matroska (MKV) format file.

Several codecs are supported (e.g. H264, VP9) are supported. Frames carry their
own time and need not arrive at regular intervals.

## A note on precise timing

During development, care was taken to ensure the time stamp of each frame is
kept with high precision (millisecond or better).

The creation time of the video is saved with nanosecond precision and the
presentation time stamp (PTS) field of each frame is stored, also with high
precision. Together (video creation time and PTS), these data allow precise
timing of each frame of data.

## Checking timestamps

To ensure the precision of the timing data in the videos saved, several
alternative approaches can be used.

Show creation time with:

    ffmpeg -i <intput_ts> -f null out.null

View timestamps with:

    mkvinfo --all <intput_ts>

or

    ffmpeg -debug_ts -re -copyts -i <intput_ts> -f null out.null

or

    ffprobe -show_packets -i <intput_ts>

## Copying to other container types without re-encoding

Note that **this can lose precise timing information**. For example, the
creation time of an MP4 video format is saved with second level, but not
nanosecond level, precision.

    ffmpeg -i original.mkv -vcodec copy -map_metadata 0:g output.mp4

## Build on Windows

```
# Download https://github.com/ShiftMediaProject/libvpx/releases/download/v1.10.0/libvpx_v1.10.0_msvc16.zip
# and unzip into %HomeDrive%%HomePath%\libvpx_v1.10.0_msvc16

set VPX_VERSION=1.10.0
set VPX_STATIC=1
set VPX_LIB_DIR=%HomeDrive%%HomePath%\libvpx_v1.10.0_msvc16\lib\x64
set VPX_INCLUDE_DIR=%HomeDrive%%HomePath%\libvpx_v1.10.0_msvc16\include
SET VPX_NO_PKG_CONFIG=1
```
