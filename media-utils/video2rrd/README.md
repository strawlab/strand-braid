# video2rrd

Convert video saved by [Strand Camera](https://strawlab.org/strand-cam/) to [Rerun](https://rerun.io/) `.rrd` file.

## Installation

Install the latest version like so:

```bash
cargo install --git https://github.com/strawlab/strand-braid video2rrd
```

## Usage

```
Convert video with Strand Cam timestamps to RRD format for Rerun Viewer

Usage: video2rrd [OPTIONS] --input <INPUT>

Options:
  -i, --input <INPUT>
          Input video filename

  -r, --recording-id <RECORDING_ID>
          Recording ID

  -e, --entity-path <ENTITY_PATH>
          Entity Path

  -c, --connect
          If true, connect directly to rerun viewer using GRPC rather than saving an output file

  -o, --output <OUTPUT>
          Output rrd filename. Defaults to "<INPUT>.rrd".

          This must not be used with --connect.

  -s, --start-time <START_TIME>
          Start time of the video. By default, this will be read from the video itself

      --exclude-before <EXCLUDE_BEFORE>
          Exclude frames before this time

      --exclude-after <EXCLUDE_AFTER>
          Exclude frames after this time

  -f, --framerate <FRAMERATE>
          Force the video to be interpreted as having this frame rate (in frames per second).

          By default, timestamps in the video itself will be used.

      --no-progress
          Disable display of progress indicator

      --undistort-with-calibration <UNDISTORT_WITH_CALIBRATION>
          Filename with camera parameters. When given, used to remove distortion in output movie.

          This allows working around https://github.com/rerun-io/rerun/issues/2499.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```
