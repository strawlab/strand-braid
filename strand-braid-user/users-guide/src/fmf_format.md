# FMF (Fly Movie Format) - simple, uncompressed movie storage format

The primary design goals of FMF files are:

 - Single pass, low CPU overhead writing of lossless movies for realtime streaming applications
 - Precise timestamping for correlation with other activities
 - Simple format that can be written and read from a variety of languages.

These goals are achieved via using a very simple format. After an initial header
containing meta-data such as image size and color coding scheme (e.g.
monochromatic 8 bits per pixel, YUV422, etc.), repeated chunks of raw image data
and timestamp are saved. Because the raw image data from the native camera
driver is saved, no additional processing is performed. Thus, streaming of
movies from camera to disk will keep the CPU free for other tasks, but it will
require a lot of disk space. Furthermore, the disk bandwidth required is
equivalent to the camera bandwidth (unless you save only a region of the images,
or if you only save a fraction of the incoming frames).

The FMF file type defines raw image sequences where each image is stored exactly
in the raw data bytes as they were acquired from the camera together with with a
timestamp. There are two versions implemented, versions 1 and 3 (Version 2 was
briefly used internally and is now best forgotten). Version 1 is deprecated and
new movies should not be written in this format.

A **Rust** implementation to read and write `.fmf` files can be found in the
[`github.com/strawlab/strand-braid`
repository](https://github.com/strawlab/strand-braid/tree/main/fmf).

Documentation for the file type and reading/writing `.fmf` files in **Python**
can be found at the [documentation of
`motmot.FlyMovieFormat`](http://code.astraw.com/projects/motmot/fly-movie-format.html).

A **MATLAB®** implementation can be found in the
[`github.com/motmot/flymovieformat`
repository](https://github.com/motmot/flymovieformat/tree/master/matlab).

An **R** implementation can be found in the [`github.com/jefferis/fmfio`
repository](https://github.com/jefferis/fmfio).

## Converting movies to and from FMF format with the `fmf` command line program

The `fmf` command line program from
[https://github.com/strawlab/strand-braid/tree/main/fmf/fmf-cli](https://github.com/strawlab/strand-braid/tree/main/fmf/fmf-cli)
can be used for a variety of tasks with `.fmf` files, especially converting to
and from other formats.

Here is the output `fmf --help`:

```
strawlab@flycube10:~$
fmf 0.1.0
work with .fmf (fly movie format) files

USAGE:
    fmf <SUBCOMMAND>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

SUBCOMMANDS:
    export-fmf       export an fmf file
    export-jpeg      export a sequence of jpeg images
    export-mkv       export to mkv
    export-png       export a sequence of png images
    export-y4m       export to y4m (YUV4MPEG2) format
    help             Prints this message or the help of the given subcommand(s)
    import-images    import a sequence of images, converting it to an FMF file
    import-webm      import a webm file, converting it to an FMF file
    info             print information about an fmf file
```

See https://github.com/strawlab/strand-braid/tree/main/fmf/fmf-cli for more information.

## File Structure

| FMF File structure |
| -------- |
| Header |
| Frame chunk 0 |
| Frame chunk 1 |
| Frame chunk 2 |
| ... |
| Frame chunk N |

The chunk size is specified in the header and is equal to the raw frame pixel
data size plus 8 bytes for the timestamp. Thus a 640 pixel wide, 480 pixel high
MONO8 format image would have a `chunksize` of 307208 (equal to 640 * 480 + 8).

Because the chunk size is constant for all frames, any chunk can be accessed by
computing its position and seeking to that location. For the same reason, the
image data within FMF files can be memory mapped.

### Header Version 3

This is the preferred header for all new FMF files.

| Start position | Type | Name | Description |
| -------- | ------- | ------------ | ------------ |
| 0 | u32 | version | Version number (3) |
| 4 | u32 | lenformat | Length of the subsequent format string |
| 8 | [u8; N] | format | ASCII string of N characters containing pixel format, e.g. `MONO8` or `YUV422` |
| 8+N |	u32 | bpp | Bits per pixel, e.g. 8 |
| 12+N | u32 | height | Number of rows of image data |
| 16+N | u32 | width | Number of columns of image data |
| 20+N | u64 | chunksize | Bytes per "chunk" (timestamp + frame) |
| 28+N | u64 | n_frames | Number of frame chunks (0=unknown, read file to find out) |

For the `format` field, the following pixel formats are known:

| Format string | Description |
| -------- | ------------ |
| `MONO8` | Monochrome data, 8 bits per pixel |
| `RAW8:RGGB` | Raw Bayer mosaic data, RGGB pattern, 8 bits per pixel |
| `RAW8:GBRG` | Raw Bayer mosaic data, GBRG pattern, 8 bits per pixel |
| `RAW8:GRBG` | Raw Bayer mosaic data, GRBG pattern, 8 bits per pixel |
| `RAW8:BGGR` | Raw Bayer mosaic data, BGGR pattern, 8 bits per pixel |
| `YUV422` | Packed YUV encoded data, 16 bits per pixel |
| `RGB8` | Packed RGB encoded data, 24 bits per pixel |

This list of pixel formats is not exhaustive and other formats can be added.

### Header Version 1

⚠ This version is deprecated and no new files with this format should be written. ⚠

Only supports MONO8 pixel format.

| Start position | Type | Name | Description |
| -------- | ------- | ------------ | ------------ |
| 0 | u32 | version | Version number (1) |
| 4 | u32 | height | Number of rows of image data |
| 8 | u32 | width | Number of columns of image data |
| 12 | u64 | chunksize | Bytes per "chunk" (timestamp + frame) |
| 20 | u64 | n_frames | Number of frames (0=unknown, read file to find out) |

### Frame Chunks

Frame chunks have an identical format in FMF v1 and v3 files. From a given
camera pixel format and size, they are constant in size and thus the Nth frame
can be accessed by seeking to `frame0_offset + n*chunksize`. The image data is
uncompressed raw image data as read directly from the camera.

| Start position within chunk | Type | Name | Description |
| -------- | ------- | ------------ | ------------ |
| 0 | f64 | timestamp | Timestamp (seconds in current epoch) |
| 8 | [u8; N] | image_data | Image data |

### Types used above

All numbers are little-endian (Intel standard).

|Type | size (in bytes) | description |
| ------- | ----------- | ------ |
| [u8; N] | N | variable length buffer of characters |
| u32 | 4 | unsigned 32 bit integer |
| u64 | 8 | unsigned 64 bit integer |
| f64 | 8 | 64 bit floating point number |
