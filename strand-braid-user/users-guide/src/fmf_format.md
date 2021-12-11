# Fly Movie Format - simple, uncompressed movie storage format

The primary design goals of FlyMovieFormat are:

 - Single pass, low CPU overhead writing of lossless movies for realtime streaming applications
 - Precise timestamping for correlation with other activities
 - Simple format that can be read from Python, C, and MATLAB®.

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
https://github.com/strawlab/strand-braid/tree/main/fmf/fmf-cli can be used for a
variety of tasks with `.fmf` files, especially converting to and from other
formats.

Here is the output `fmf --help`

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

## File structure

- header (either version 1 or version 3)
- arbitrary number of timestamp and frame pairs (“frame chunks”)

### .fmf header version 3

| Typecode |	Name |	Description |
| -------- | ------- | ------------ |
| I  |  version |	Version number (3) |
| I  |  lenformat |	Length of the subsequent format string |
| \*B | 	format |	string containing format, e.g. ‘MONO8’ or ‘YUV422’ |
| I  |	bpp |	Bits per pixel, e.g. 8 |
| II | 	framesize |	Number of rows and columns in each frame |
| Q  |	chunksize |	Bytes per “chunk” (timestamp + frame) |
| Q  |	n_frames |	Number of frames (0=unknown, read file to find out) |

### .fmf header version 1
This version is deprecated and no new files with this format should be written.

Only supports MONO8 format.

| Typecode | Name      | Description |
| -------- | --------- | ----------- |
| I 	   | version   |	Version number (1) |
| II 	   | framesize |	Number of rows and columns in each frame |
| Q    	   | chunksize |	Bytes per “chunk” (timestamp + frame) |
| Q 	   | n_frames  |	Number of frames (0=unknown, read file to find out) |

### .fmf frame chunks

| Typecode |	Name 	| Description |
| ----     | ---------- | ---------   |
|d |	timestamp |	Timestamp (seconds in current epoch)|
|\*B |	frame |	Image data, rows*columns bytes, row major ordering|

### Typecodes used above

All numbers are little-endian (Intel standard).

|Typecode |	description |	bytes |	C type |
| ------- | ----------- | ------- | ------ |
|B 	uint8 |	1 |	unsigned char |
|I 	uint32 |	4 |	unsigned int |
|Q 	uint64 |	8 |	unsigned long long (__int64 on Windows) |
|d 	double64 |	8 |	double |
|\*B |	data 	|  	an unsigned char buffer of arbitrary length |
