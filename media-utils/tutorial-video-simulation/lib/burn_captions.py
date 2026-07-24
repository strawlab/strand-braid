#!/usr/bin/env python

# Overlays timed on-screen captions (recorded by session.sh's log_event) onto
# a screen capture produced by session.sh's start_capture, matching the
# silent, caption-only style of the original tutorial videos this directory
# regenerates (e.g. a "Ctrl+C" label at the moment that key was sent, rather
# than narration audio).
#
# No third-party dependencies -- run with plain python3.
#
# Usage:
#   python3 lib/burn_captions.py \
#       --events events.jsonl --input raw.mp4 --output final.mp4

import argparse
import json
import os
import subprocess
import sys
import tempfile


def build_filter(events, text_dir):
    # Each caption's raw text is written to its own file and referenced via
    # drawtext's `textfile` option, rather than inlined via `text='...'` --
    # ffmpeg's drawtext filter does its own SECOND layer of escaping on top
    # of the filtergraph's own quoting rules ("double escaping", per
    # ffmpeg's own docs), and getting both layers right for an arbitrary
    # caption containing a literal single quote turned out to be fragile in
    # practice: a caption whose text contained a shell-quoted path
    # (`braid-run '/path/to/config.TOML'`) first broke the filtergraph
    # parser outright ("No such filter: '<value>'", from an unescaped
    # comma), and once that was fixed by properly splicing the quote
    # ('\'' -- the same trick POSIX shells use), the quote characters
    # silently vanished from the rendered caption instead of appearing
    # literally, i.e. drawtext's own escaping layer was consuming them.
    # `textfile` sidesteps both layers: the file's bytes are rendered
    # verbatim, with no escaping of the caption text itself needed at all --
    # only the file PATH goes through the filtergraph's own quoting, and
    # since we generate that path ourselves (a plain tempdir/caption_N.txt),
    # it never contains anything that needs escaping.
    parts = []
    for i, ev in enumerate(events):
        start = float(ev["t"])
        end = start + float(ev["duration"])
        text_path = os.path.join(text_dir, f"caption_{i}.txt")
        with open(text_path, "w") as f:
            f.write(ev["text"])
        parts.append(
            "drawtext="
            f"textfile='{text_path}':"
            # fontsize/borderw/x/y scaled 1.5x along with session.sh's
            # SESSION_WIDTH/HEIGHT (1280x800 -> 1920x1200) to stay the same
            # size relative to the frame -- rescale these too if that ever
            # changes again.
            "fontcolor=yellow:fontsize=54:borderw=3:bordercolor=black:"
            "x=60:y=h-th-60:"
            f"enable='between(t,{start},{end})'"
        )
    return ",".join(parts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--events", required=True, help="events.jsonl from session.sh's log_event")
    parser.add_argument("--input", required=True, help="raw screen-capture .mp4 from start_capture")
    parser.add_argument("--output", required=True, help="captioned .mp4 to write")
    parser.add_argument(
        "--comment",
        help="written into the output mp4's 'comment' metadata tag (e.g. the strand-cam version used)",
    )
    args = parser.parse_args()

    events = []
    with open(args.events) as f:
        for line in f:
            line = line.strip()
            if line:
                events.append(json.loads(line))

    with tempfile.TemporaryDirectory(prefix="burn_captions-") as text_dir:
        cmd = ["ffmpeg", "-y", "-i", args.input]
        if events:
            cmd += ["-vf", build_filter(events, text_dir)]
        if args.comment:
            cmd += ["-metadata", f"comment={args.comment}"]
        cmd += ["-c:a", "copy", args.output]

        print("+", " ".join(cmd), file=sys.stderr)
        subprocess.run(cmd, check=True)


if __name__ == "__main__":
    main()
