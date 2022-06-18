# Processing recorded videos using Braid

## Overview

The `braid-process-video` program will takes video files and process them to
produce various outputs.

![braid-process-video.png](braid-process-video.png)

As shown in the figure, this program takes `.mkv` (or `.fmf`) video input files
saved by Braid and a configuration file and then creates an output `.mkv` video
which stitches the input videos together. Optionally, it can also plot 2D
detections from a `.braidz` file on top of the raw video.

## Note

- The input videos must be saved by Braid to ensure that the timestamps for each
  frame in the file are correctly stored.

## Example usage 1: Automatic determination of inputs

If a directory contains a single `.braidz` file and one or more video files,
`braid-process-video` can automatically generate a video with default options.
In this case run it like so:

```ignore
braid-process-video auto-config --input-dir /path/to/video-and-braidz-files
```

## Example usage 2: Use of a `.toml` configuration file

Here is an example configuration file `braid-bundle-videos.toml`:

```ignore
# The .braidz file with 2D detection data (optional).
input_braidz = "20211011_163203.braidz"

# This stanza specified that an output video will be made.
[[output]]
type = 'video'
filename = 'composite.mkv'

# The following sections specify video sources to use as input.
[[input_video]]
filename = 'movie20211011_163224.mkv'

[[input_video]]
filename = 'movie20211011_163228.mkv'
```

With such a configuration file, run the program like so:

```ignore
braid-process-video config-toml --config-toml braid-bundle-videos.toml
```

## TODO

There are many more options which can be configured in the `.toml` configuration
file and they should be documented.
