## Should there even be an FMF v 4?

Should there even be an FMF v 4? Why not use a standard video container which
supports timestamps? For the question of whether a standard video container that
supports timestamps, see
[this](https://video.stackexchange.com/questions/34298).

## webm and matroska

The matroska container (MKV) and the webm subset can save timestamps for start
of segment with the DateUTC metadata. Each further frame has a time offset in
the track. This can be used to reconstruct timestamps just as accurately as fmf.

Therefore, we could use .mkv uncompressed to save raw video and .mkv or .webm to
save compressed video. [FFV1](https://github.com/FFmpeg/FFV1) is a lossless
codec. [JPEG 2000](https://en.wikipedia.org/wiki/JPEG_2000) supports lossless
single frames. Also the `V_UNCOMPRESSED` codec type is mentioned in the
[Matroska docs](https://matroska.org/technical/specs/codecid/index.html).

The main remaining problem is that no codec seems to encode raw bayer ("CFA =
Color Filter Array") data lossless uncompressed. Here is a [PR for FFV1 to add
support](https://github.com/FFmpeg/FFV1/pull/100). Given for now that most of
what we care about is mono, we can probably defer dealing with this for now.

Can make example FFV1 in MKV file with this ([as explained
here](https://github.com/MediaArea/MediaConch/wiki/HowTo)):

    ffmpeg -f lavfi -i mandelbrot -t 1 -c:v ffv1 -level 3 -g 1 test_ffv1_v3.mkv

Is the perfect code this: https://github.com/MediaArea/RAWcooked ?

Wait, now I discovered that VP9 has lossless mode. This changes almost
everything...

### Random notes from gstreamer about saving timestamps

For example the documentation for the videorate gstreamer
pluging suggests that such a thing exists but is not Ogg or AVI: "Typical
examples are formats that do not store timestamps for video frames, but only
store a framerate, like Ogg and AVI". When using gstreamer, presumably we want
[GstReferenceTimestampMeta](https://gstreamer.freedesktop.org/data/doc/gstreamer/head/gstreamer/html/GstBuffer.html#GstReferenceTimestampMeta).
The decklink gstreamer plugin seems to take advantage of this, e.g. the
gstdecklinkvideosrc.cpp file. Also, we would then use the "x-raw" video encoding
type or perhaps actually compress into e.g. "x-h265". Allegedly Matroska (aka
.webm?) supports timestamps. For example,
https://www.linuxtv.org/wiki/index.php/GStreamer writes "If you choose a
container format that supports timestamps (e.g. Matroska), timestamps are
automatically written to the file and used to vary the playback speed". See
[this](http://gstreamer-devel.966125.n4.nabble.com/time-of-day-timestamps-handling-in-gstreamer-v4l2-source-td4668411.html).
There is this quote: "Note that to me this is all a bit awkward. I'd would
forget about getting the v4l2 timestamp, implement and set a UTC/NTP clock on my
pipleine and save the base_time for each generated streams. This way, when I
play back the file, I know that the NTP time I want is the streaming +
ntp-base-time."

### What codec for recording raw Bayer data?

Here is more interesting info from [here](https://lists.ffmpeg.org/pipermail/ffmpeg-user/2015-May/026623.html)

    Here's what seems to have worked for me:

    ffmpeg -f v4l2 -framerate 60 -video_size 1920x1080 -ts mono2abs -i
    /dev/video-static -r 2997/100 -f matroska -c:v nvenc -b:v 25000k -minrate
    25000k -maxrate 25000k -g 1 -profile:v high -preset hq -copyts output.mkv

    It took me a few tries to get the -ts mono2abs and -copyts into the right
    location, but now I believe I'm getting good wall-clock time (at least, it
    looks logical to me). I'm still doing some testing, though...

All in all, there seems to be no lossless encoder that natively supports Bayer
images. Recommended is to juxtapose each plane into one larger image which is
easier for lossless compression than the Bayer mosaic.

### Live streaming to mkv with ffmpeg/x264

[Here is an example "FFmpeg Live Stream Recording x264-aac-mkv on Bionic
Beaver"](https://ubuntuforums.org/showthread.php?t=2402676) of using ffmpeg to
pipe livestream data to an .mkv file and x264 for encoding.

See also ["Low-Latency Live Streaming your Desktop using
ffmpeg"](http://fomori.org/blog/?p=1213).

[This page](https://trac.ffmpeg.org/wiki/StreamingGuide) is also useful about
using FFMPEG for live streaming.
