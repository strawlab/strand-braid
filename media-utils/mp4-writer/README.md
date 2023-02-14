# mp4-writer

Save video to an MP4 format file.

Currently, only the H264 codec is supported. Frames carry their
own time and need not arrive at regular intervals.

## A note on precise timing

If the `insert_precision_timestamp` argument to `Mp4Writer::write_h264_buf` is
set, the time stamp of the frame is saved with microsecond precision in the H264
stream according to the Motion Imagery Standards Board Standard 0604 Precision
Time Stamps.
