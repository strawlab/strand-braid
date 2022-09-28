# mp4-writer

Save video to an MP4 format file.

Currently, only the H264 codecs is supported. Frames carry their
own time and need not arrive at regular intervals.

## A note on precise timing

During development, care was taken to ensure the time stamp of each frame is
kept with high precision (millisecond or better). The timestamps are saved in
the H264 stream according to the Motion Imagery Standards Board Standard 0604
Precision Time Stamps. Each frame is timestamped to microsecond precision.

## Checking timestamps

To ensure the precision of the timing data in the videos saved, several
alternative approaches can be used.

View timestamps with:

    ffmpeg -debug_ts -re -copyts -i <intput_ts> -f null out.null

or

    ffprobe -show_packets -i <intput_ts>

Extract h264 to raw "annex B" format:

    ffmpeg -i animation.mp4 -vcodec copy -an -bsf:v h264_mp4toannexb animation.h264
