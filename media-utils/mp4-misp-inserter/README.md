# mp4-misp-inserter

Insert per-frame MISPmicrosectime precision-timestamp SEI NAL units into an
MP4's H.264 stream, without decoding or re-encoding the video.

The timestamps to embed are read from a per-frame timing source -- by default
a companion SubRip (`.srt`) subtitle file, as written alongside Strand Camera
recordings -- and spliced into the existing H.264 samples as new NAL units
while the samples themselves are copied through unchanged. The container's
original per-sample timing (`stts`/`ctts`) is preserved verbatim, so this
works correctly even for reordered (B-frame) streams.

This is useful when a video was recorded without embedded precision
timestamps (e.g. by a plain ffmpeg-based recorder) but a `.srt` file with the
per-frame capture times exists alongside it, and you want a single MP4 that
carries the capture time in-band, as read by
[`show-timestamps`](../show-timestamps) with `--timestamp-source
misp-microsectime` or by `frame_source::TimestampSource::MispMicrosectime`.

## Compilation and installation

The `mp4-misp-inserter` program is packaged and installed by the
`strand-braid` installer.

Alternatively, it can be installed using standard Rust tools. Here are
instructions about how to [install
Rust](https://www.rust-lang.org/tools/install). Once this is done, you can
install `mp4-misp-inserter` like this:

```bash
cd <path_to_strand_braid>/media-utils/mp4-misp-inserter
cargo install --path .
```

## Usage

```
Usage: mp4-misp-inserter [OPTIONS] <INPUT>

Arguments:
  <INPUT>
          Input MP4 file

Options:
      --output <OUTPUT>
          Output MP4 file. Defaults to the input's path with `-misp` inserted before the `.mp4` extension

      --srt <SRT>
          SRT file with per-frame timestamps. Only used when `--timestamp-source srt-file` (the default). Defaults to the input's path with its extension changed to `.srt`

      --timestamp-source <TIMESTAMP_SOURCE>
          Source of the per-frame timestamps to embed as MISP SEI

          [default: srt-file]
          [possible values: best-guess, frame-info-recv-time, mp4-pts, srt-file]

      --force
          Overwrite the output file if it already exists

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

## Example usage

Given `movie.mp4` and a companion `movie.srt` (as `show-timestamps --output
srt` would produce, or as written alongside a Strand Camera recording), embed
the `.srt` timestamps as MISP SEI into a new `movie-misp.mp4`:

```bash
mp4-misp-inserter movie.mp4
```

Choose an explicit output path and overwrite it if it already exists:

```bash
mp4-misp-inserter movie.mp4 --output movie-with-timestamps.mp4 --force
```

Verify the result carries the embedded timestamps:

```bash
show-timestamps --timestamp-source misp-microsectime movie-misp.mp4
```
