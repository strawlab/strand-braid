# mp4-bframe-doctor

Detect MP4 or raw Annex B `.h264` files whose timing metadata is inconsistent
with the true presentation order encoded in the H.264 bitstream itself.

There are two places a recording states timing, and either can be wrong:

* the MP4 container's `stts`/`ctts` boxes, which is what a player uses to order
  frames; and
* the per-frame precision-timestamp SEI embedded in the bitstream (written by
  strand-cam / braid), which can itself be mistagged at record time (e.g.
  paired with the wrong encoder output when B-frame reordering delays that
  output relative to when it was submitted), independent of what the container
  boxes say.

The one signal that cannot lie is the bitstream's own picture order count
(POC, ITU-T H.264 §8.2.1): every slice header carries enough information to
reconstruct the true relative display order of samples, independent of any
container metadata or of what a (possibly buggy) writer put in the SEI. This
tool decodes POC for every sample and checks that, walked in POC order, each
available timing series comes out non-decreasing. If a series is not monotonic
in POC order, that timing disagrees with the bitstream's real display order and
the file is broken.

Which series are checked:

* the container timing is checked for **every MP4** (so even a plain
  ffmpeg-encoded recording with no SEI can be verified); and
* the precision-timestamp SEI is checked wherever it is present -- for a raw
  Annex B `.h264` file it is the only available signal.

Streams using `pic_order_cnt_type` 0 or 2 are supported (this covers
essentially all cameras and software/hardware H.264 encoders in practice); the
rare type 1 is reported as `UNKNOWN`.

## Repair (`fix`)

The `fix` subcommand repairs an affected file **in place**. Because the SEI
itself is untrustworthy, the only lie-proof signal is the bitstream's picture
order count (POC). `fix` assumes the *set* of capture timestamps in the file is
correct but was permuted onto the wrong frames, and that the camera captured
frames in display order. It therefore sorts the capture times and reassigns
them to frames by POC (display) rank, then writes a new MP4 that keeps the
original decode-order bitstream but lays down composition offsets (`ctts`) and
a fresh per-frame precision-timestamp SEI so that the container order and the
SEI both agree with the true display order. The old (stale) precision-timestamp
SEI is stripped so only the corrected one remains.

The original file `X` is renamed to `X.bak` (or `X.bak.1`, `X.bak.2`, ... if a
backup already exists) and the repaired file is written to `X`. Already-OK
files are left untouched unless `--force` is given.

The repaired output is always MP4 data (Annex B has no container to carry
`ctts`), so fixing a raw `X.h264` writes MP4 bytes to `X.h264` — rename it to
`.mp4` afterwards if the extension matters to you.

## Compilation and installation

The `mp4-bframe-doctor` program is packaged and installed by the
`strand-braid` installer.

Alternatively, it can be installed using standard Rust tools. Here are
instructions about how to [install
Rust](https://www.rust-lang.org/tools/install). Once this is done, you can
install `mp4-bframe-doctor` like this:

```bash
cd <path_to_strand_braid>/media-utils/mp4-bframe-doctor
cargo install --path .
```

## Usage

```
Usage: mp4-bframe-doctor <COMMAND>

Commands:
  check  Report whether the SEI precision timestamps in MP4 or raw Annex B .h264 files are consistent with the true presentation order encoded in the H.264 bitstream (its picture order count, POC)
  fix    Repair a file in place by reassigning its capture timestamps to frames in true (bitstream POC) display order and writing a new MP4 whose container timing and SEI both agree with that order
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

```
Usage: mp4-bframe-doctor check <INPUTS>...
Usage: mp4-bframe-doctor fix [--force] <INPUT>
```

## Example usage

Scan a directory of recordings for the bug, exiting non-zero if any are found:

```bash
mp4-bframe-doctor check /some_path/*.mp4
```

Both MP4 and raw Annex B `.h264` files are accepted (dispatched by extension):

```bash
mp4-bframe-doctor check recording.mp4 recording.h264
```

Repair a broken recording in place (the original is kept as `recording.mp4.bak`):

```bash
mp4-bframe-doctor fix recording.mp4
```

## Caveat

An MP4 can always be checked against its container timing. The extra SEI check
relies on the precision-timestamp SEI that strand-cam / braid embeds when
writing MP4s through the `ffmpeg-rewriter` path. A raw Annex B `.h264` file has
no container, so if it also lacks that SEI there is nothing to check and it is
reported as `UNKNOWN` rather than silently assumed to be fine.
