# mp4-bframe-doctor

Detect MP4 files whose per-frame precision-timestamp SEI data (as written by
strand-cam / braid) is inconsistent with the true presentation order encoded
in the H.264 bitstream itself.

A container's `stts`/`ctts` boxes are one place a recording can claim the
wrong presentation order, but they aren't the only place: the SEI timestamp
embedded in each sample can itself be mistagged at record time (e.g.
associated with the wrong encoder output when B-frame reordering delays that
output relative to when it was submitted for encoding), independent of what
the container boxes say. Such a file has no trustworthy timing metadata left
in the container at all: neither `ctts` nor the SEI can be assumed correct.

The one signal that cannot lie is the bitstream's own picture order count
(POC, ITU-T H.264 §8.2.1): every slice header carries enough information to
reconstruct the true relative display order of samples, independent of any
container metadata or of what a (possibly buggy) writer put in the SEI. This
tool decodes POC for every sample and checks whether sorting samples by POC
reproduces non-decreasing SEI timestamps. If not, the SEI data is
inconsistent with the bitstream's real presentation order and the file is
broken.

Only `pic_order_cnt_type == 0` streams are supported (covers essentially all
cameras and software/hardware H.264 encoders in practice); others are
reported as `UNKNOWN`.

## Status

Currently `check`-only. A `fix` subcommand existed for an earlier, narrower
bug (a missing `ctts` box, with trustworthy SEI data) but its repair strategy
doesn't apply to what `check` detects now: if the SEI itself is mistagged,
there is no trustworthy per-frame timestamp left in the container to repair
from. The old implementation is kept, commented out, in `src/main.rs`.

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
  check  Report whether the SEI precision timestamps in MP4 files are consistent with the true presentation order encoded in the H.264 bitstream (its picture order count, POC)
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

```
Usage: mp4-bframe-doctor check <INPUTS>...
```

## Example usage

Scan a directory of recordings for the bug, exiting non-zero if any are found:

```bash
mp4-bframe-doctor check /some_path/*.mp4
```

## Caveat

Detection relies on the precision-timestamp SEI that strand-cam / braid
always embeds when writing MP4s through the `ffmpeg-rewriter` path. Files
without it can't be analyzed and are reported as `UNKNOWN` rather than
silently assumed to be fine.
