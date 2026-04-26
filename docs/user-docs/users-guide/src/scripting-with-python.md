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

## Advanced: automating manual actions

Any action available in the browser UI can be scripted. To discover the
corresponding HTTP call, open your browser's developer tools (F12 in most
browsers), go to the **Network** tab, and perform the action manually. You will
see the POST request sent to the `/callback` endpoint and the JSON body it
carries. You can then replicate that call in Python using the `StrandCamProxy`
or `BraidProxy` pattern shown in the demo scripts above.
