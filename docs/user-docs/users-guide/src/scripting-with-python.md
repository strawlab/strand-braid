# Scripting with Python

Everything in Strand Camera and Braid that can be controlled from the web
browser can also be controlled from a Python script. The general technique is to
connect to a running Strand Camera (or Braid) process over HTTP, exactly as a
browser does. All scripts below require the
[`requests`](https://docs.python-requests.org/en/latest/user/install) library:

```sh
pip install requests
```

## Demo: recording a video using Strand Camera from a Python script

[`record-mp4-video.py`](https://github.com/strawlab/strand-braid/blob/main/docs/user-docs/scripts/record-mp4-video.py)
connects to a running Strand Camera instance, sends a command to start MP4
recording, waits five seconds, then sends a command to stop. Run it like so:

```sh
python record-mp4-video.py --strand-cam-url http://127.0.0.1:3440/
```

The `--strand-cam-url` argument defaults to `http://127.0.0.1:3440/` and should
be changed to match the URL shown in your Strand Camera window. Modify the
`time.sleep(5.0)` call to change the recording duration, or replace the fixed
sleep with your own experimental logic between the start and stop commands.

## Demo: recording a color video with the ffmpeg codec

[`record-mp4-video-ffmpeg.py`](https://github.com/strawlab/strand-braid/blob/main/docs/user-docs/scripts/record-mp4-video-ffmpeg.py)
records an MP4 using the **Ffmpeg** codec, which pipes frames to the system
[`ffmpeg`](https://ffmpeg.org/) binary. Unlike the built-in H.264 encoders, the
ffmpeg codec accepts color input, so this is the path to use for color
recordings — for example on GPUs where the built-in NVENC encoder is
unavailable. Start Strand Camera with a color pixel format:

```sh
strand-cam --camera-name my-camera --pixel-format RGB8
```

and then run:

```sh
python record-mp4-video-ffmpeg.py --strand-cam-url http://127.0.0.1:3440/ --codec libx264
```

The `--codec` argument selects the ffmpeg encoder (default `libx264`; other
common choices are `h264_nvenc` and `h264_vaapi`) and must be available in your
`ffmpeg` build. Pass `--verify-dir` with the directory Strand Camera writes to
(its `--data-dir`) to have the script confirm that a non-empty `.mp4` was
produced; this is how the script is exercised as an automated end-to-end test in
the project's continuous integration, so it stays in sync with the software.

## Demo: recording multiple videos using Braid from a Python script

[`record-mp4-video-braid-all-cams.py`](https://github.com/strawlab/strand-braid/blob/main/docs/user-docs/scripts/record-mp4-video-braid-all-cams.py)
connects to a running Braid instance and sends a single command that starts MP4
recording on all connected cameras simultaneously, waits five seconds, then
stops. Run it like so:

```sh
python record-mp4-video-braid-all-cams.py --braid-url http://127.0.0.1:8397/
```

The `--braid-url` argument defaults to `http://127.0.0.1:8397/`. As with the
single-camera script, replace the fixed sleep with your own trigger logic to
control exactly when recording starts and stops.

## Demo: save preview images to disk from Strand Camera using Python

[`strand_cam_subscriber.py`](https://github.com/strawlab/strand-braid/blob/main/docs/user-docs/scripts/strand_cam_subscriber.py)
subscribes to the live image stream from Strand Camera and saves each frame as
a JPEG file (`image0000.jpg`, `image0001.jpg`, …). Run it like so:

```sh
python strand_cam_subscriber.py --strand-cam-url http://127.0.0.1:3440/
```

Each frame is acknowledged back to Strand Camera after it is saved, which acts
as flow control — Strand Camera will not send the next frame until the
acknowledgment is received. This makes the script suitable as a starting point
for per-frame image processing in Python.

## Demo: listen to realtime 3D tracking data using Python

[`braid_retransmit_udp.py`](https://github.com/strawlab/strand-braid/blob/main/docs/user-docs/scripts/braid_retransmit_udp.py)
subscribes to the live event stream from Braid and re-transmits the 3D position
of each tracked object as a UDP packet containing comma-separated `x, y, z`
values. Run it like so:

```sh
python braid_retransmit_udp.py --braid-url http://127.0.0.1:8397/ \
    --udp-host 127.0.0.1 --udp-port 1234
```

The event stream carries three types of messages: `Birth` (a new object is
first detected), `Update` (position estimate for a tracked object), and `Death`
(an object is no longer tracked). The script forwards only `Update` events. You
can extend it to handle `Birth` and `Death` events for applications that need
to track object identity over time.

## Demo: resetting the object detection background model using Python

[`reset-background.py`](https://github.com/strawlab/strand-braid/blob/main/docs/user-docs/scripts/reset-background.py)
presses the background model buttons of the [object detection
UI](parameters_for_object_detection_and_tracking.md) programmatically. By
default it acts like the **Take Current Image As Background** button,
re-initializing the background model from the next ~20 incoming frames:

```sh
python reset-background.py --strand-cam-url http://127.0.0.1:3440/
```

With `--clear-to-value`, it instead acts like the **Set background to
mid-gray** button, setting the background model to a uniform gray value:

```sh
python reset-background.py --strand-cam-url http://127.0.0.1:3440/ --clear-to-value 127
```

The underlying HTTP calls are simple: a POST to the `/callback` endpoint with
the JSON body `"TakeCurrentImageAsBackground"` or `{"ClearBackground": 127.0}`.

## Demo: resetting the background model on all cameras of a Braid setup

[`reset-background-braid-all-cams.py`](https://github.com/strawlab/strand-braid/blob/main/docs/user-docs/scripts/reset-background-braid-all-cams.py)
performs the same background reset on every camera connected to a running
Braid instance. Run it like so:

```sh
python reset-background-braid-all-cams.py --braid-url http://127.0.0.1:8397/
```

Unlike MP4 recording, there is no single Braid command for this. Instead, the
script asks Braid for the HTTP address of every connected camera (this is
carried in the `connected_cameras` field of the Braid event stream) and then
sends the command to each Strand Camera directly. The same pattern can be used
to automate any other per-camera action across a whole Braid setup.

## Advanced: automating manual actions

Any action available in the browser UI can be scripted. To discover the
corresponding HTTP call, open your browser's developer tools (F12 in most
browsers), go to the **Network** tab, and perform the action manually. You will
see the POST request sent to the `/callback` endpoint and the JSON body it
carries. You can then replicate that call in Python using the `StrandCamProxy`
or `BraidProxy` pattern shown in the demo scripts above.
