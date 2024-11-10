# Scripting with Python

Everything in Strand Cam and Braid that can be controlled from the web browser
can also be controlled from a Python script. The general technique is to use a
Python library to connect to a running Strand Cam (or Braid) program exactly
like a web browser does it.

## Demo: recording a video using Strand Camera from a Python script

TODO: describe how to use and modify the [`record-mp4-video.py`
demo](https://github.com/strawlab/strand-braid/blob/main/strand-braid-user/scripts/record-mp4-video.py).

## Demo: recording multiple videos using Braid from a Python script

TODO: describe how to use and modify the [`record-mp4-video-braid-all-cams.py`
demo](strand-braid-user/scripts/record-mp4-video-braid-all-cams.py).

## Demo: save preview images to disk from Strand Camera using Python

TODO: describe how to use and modify the [`strand_cam_subscriber.py`
demo](https://github.com/strawlab/strand-braid/blob/main/strand-braid-user/scripts/strand_cam_subscriber.py).

## Demo: listen to realtime 3D tracking data using Python

TODO: describe how to use and modify the [`braid_retransmit_udp.py`
demo](https://github.com/strawlab/strand-braid/blob/main/strand-braid-user/scripts/braid_retransmit_udp.py).

## Advanced: automating manual actions

TODO: describe how to use the developer tools to watch the network requests from
your browser to view HTTP POST callbacks taken on certain actions.

## Advanced: Running Strand Cam within Python

It is also possible to run strand cam within a Python program. This allows, for
example, to analyze images from within a Python script with minimal latency. See
the
[py-strandcam](https://github.com/strawlab/strand-braid/tree/main/py-strandcam)
directory.
