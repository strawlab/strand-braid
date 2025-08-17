# burn-timestamps

Burn timestamps contained in mp4 video into a new video file.

## Installation

Install the latest version like so:

```bash
cargo install --git https://github.com/strawlab/strand-braid burn-timestamps
```

## Usage

```
Usage: burn-timestamps [OPTIONS] --input <INPUT> --output <OUTPUT>

Options:
      --input <INPUT>
          Input MP4 video
      --output <OUTPUT>
          Output MP4 file
      --timestamp-source <TIMESTAMP_SOURCE>
          Source of timestamp [default: best-guess] [possible values: best-guess, frame-info-recv-time, mp4-pts, misp-microsectime, srt-file]
  -n, --no-progress
          Disable showing progress
  -h, --help
          Print help
  -V, --version
          Print version
```
