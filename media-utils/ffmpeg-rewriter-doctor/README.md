# ffmpeg-rewriter-doctor

Finish MP4 recordings which were interrupted by a crash.

Strand Camera and Braid write a recording as `<name>.mp4.partial` and rename it
to `<name>.mp4` only once it is finished. A file named `<name>.mp4` is therefore
always a finished recording, with its capture times inside it. Everything
unfinished ends in `.partial`.

Through ffmpeg, they record in two stages:

1. ffmpeg encodes the video into `<name>.mp4.partial`, while each frame's capture
   time is logged to a temporary sidecar file, `<name>-ffmpeg-rewriter.srt` (any
   camera metadata goes to `<name>-metadata.json`); and
2. when the recording is closed, that MP4 is re-muxed — without transcoding —
   into `<name>.mp4-rewritten.partial`, so every frame carries its capture time
   as a precision-timestamp SEI NAL unit. Once that is complete,
   `<name>.mp4.partial` is deleted, the result is renamed to `<name>.mp4`, and
   the sidecar files are deleted.

A crash, power loss or `kill -9` in between leaves stage 2 undone. The video is
all there, but its timestamps are only in the sidecar file rather than inside the
MP4 where every other tool looks for them. Those leftover files are how such a
recording is recognized:

```
2024-05-04_09-30-00_cam1.mp4.partial            the video, no timestamps in it
2024-05-04_09-30-00_cam1-ffmpeg-rewriter.srt    the capture times
2024-05-04_09-30-00_cam1-metadata.json          the camera metadata
2024-05-04_09-30-00_cam1.mp4-rewritten.partial  output of stage 2, if it had started
```

This tool finishes stage 2 for such recordings, producing the same MP4 the
interrupted recording would have. Which files are there says how far the
recording had got:

- With `<name>.mp4.partial` there, stage 2 was not finished. A
  `-rewritten.partial` beside it is discarded rather than salvaged: an
  interrupted writer never got to write the sample index its output needs,
  whereas the inputs stage 2 reads are still intact, so the stage is simply
  redone.
- With only `<name>.mp4-rewritten.partial` there, stage 2 was finished, because
  `<name>.mp4.partial` is deleted only then. It is renamed to `<name>.mp4`.

Name a recording to this tool by the name it has when finished, `<name>.mp4`,
or by any of its leftover files.

## Recordings made by the other encoders

Strand Camera's built-in encoders embed the capture times as they record, so
they need no second stage and write no sidecar files. They, too, write
`<name>.mp4.partial` and rename it when the recording is finished. If such a file
is in fact a finished recording, which happens when recording stops with an
error after the file could still be closed properly, this tool recognizes it by
the capture times inside it and renames it to `<name>.mp4`. One which was never
closed is reported as not finalized, as below.

## What cannot be recovered

The capture-time sidecar is written one frame behind, because a frame's entry
needs the *next* frame's time to close it. The capture time of the frame in
flight when the crash happened was therefore never written, and frames are only
kept in whole groups of pictures (a partial group cannot be reordered correctly
without the timing of every frame in it). So the last group of pictures is lost
— typically a fraction of a second, but it depends on the keyframe interval the
encoder was using. How many frames were kept is always reported.

A recording whose MP4 was never finished (no `moov` box, because ffmpeg was
killed too rather than being left to drain its input, or because Strand Camera
was killed while recording with a built-in encoder) cannot be repaired by this
tool; it is reported rather than touched. A tool such as
[`untrunc`](https://github.com/ponchio/untrunc) may be able to rebuild the index
first.

## Running while recording

A recording still in progress has no `moov` box either, so it is reported as not
finalized and left alone. Running `fix` over a directory that is being recorded
into is therefore safe, but the recordings still in progress will be reported as
errors.

## Compilation and installation

The `ffmpeg-rewriter-doctor` program is packaged and installed by the
`strand-braid` installer.

Alternatively, it can be installed using standard Rust tools. Here are
instructions about how to [install
Rust](https://www.rust-lang.org/tools/install). Once this is done, you can
install `ffmpeg-rewriter-doctor` like this:

```bash
cd <path_to_strand_braid>/media-utils/ffmpeg-rewriter-doctor
cargo install --path .
```

## Usage

```
Usage: ffmpeg-rewriter-doctor <COMMAND>

Commands:
  check  Report which recordings were interrupted, changing nothing. Exits non-zero if any need repair
  fix    Finish interrupted recordings in place: embed the capture times from the sidecar into the MP4 and remove the leftover files
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

```
Usage: ffmpeg-rewriter-doctor check <INPUTS>...
Usage: ffmpeg-rewriter-doctor fix <INPUTS>...
```

Each input is either a directory, which is searched recursively for interrupted
recordings, or a single file. A file may be the recording itself
(`movie.mp4`) or one of its leftover files (`movie-ffmpeg-rewriter.srt`,
`movie.mp4-rewritten.mp4`), whichever is at hand.

## Example usage

Look for interrupted recordings under a directory of recordings, exiting
non-zero if any are found:

```bash
ffmpeg-rewriter-doctor check /some_path/
```

Repair everything it found:

```bash
ffmpeg-rewriter-doctor fix /some_path/
```

```
REPAIRED  /some_path/2024-05-04_09-30-00_cam1.mp4  (kept 1480 of 1493 frames: the timestamp sidecar "/some_path/2024-05-04_09-30-00_cam1-ffmpeg-rewriter.srt" has capture times for only 1492 of 1493 frames, kept 1480 (whole groups of pictures only))
```

Repair one recording, named by the stray file that drew your attention to it:

```bash
ffmpeg-rewriter-doctor fix /some_path/movie.mp4-rewritten.mp4
```

Repairing an already-repaired (or never-interrupted) recording does nothing, so
`fix` is safe to re-run over a whole directory.

## Checking the result

The repaired MP4 carries its capture times the same way a normally-closed
recording does, so the usual tools can inspect it:

```bash
show-timestamps --timestamp-source misp-microsectime /some_path/movie.mp4
```
