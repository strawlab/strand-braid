# Scripting with Python

Everything in Strand Cam and Braid that can be controlled from the web browser
can also be controlled from a Python script. The general technique is to use a
Python library to connect to a running Strand Cam (or Braid) program exactly
like a web browser does it.

## Demo: changing tracking settings from a Python script

TODO: describe how to use and modify the [`record-mp4-video.py`
demo](https://github.com/strawlab/strand-braid/blob/main/strand-braid-user/scripts/record-mp4-video.py).

## Advanced: automating manual actions

TODO: describe how to use the developer tools to watch the network requests from
your browser to view HTTP POST callbacks taken on certain actions.

## Advanced: Running Strand Cam within Python

It is also possible to run strand cam within a Python program. This allows, for
example, to analyze images from within a Python script with minimal latency. See
the
[py-strandcam](https://github.com/strawlab/strand-braid/tree/main/py-strandcam)
directory.
