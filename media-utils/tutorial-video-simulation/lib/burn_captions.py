#!/usr/bin/env python

# Overlays timed on-screen captions (recorded by session.sh's log_event) onto
# a screen capture produced by session.sh's start_capture, matching the
# silent, caption-only style of the original tutorial videos this directory
# regenerates (e.g. a "Ctrl+C" label at the moment that key was sent, rather
# than narration audio).
#
# Usage:
#   uv run --no-project lib/burn_captions.py \
#       --events events.jsonl --input raw.mp4 --output final.mp4

import argparse
import json
import subprocess
import sys


def escape_drawtext(text):
    # ffmpeg drawtext treats : and \ and ' specially.
    return text.replace("\\", "\\\\").replace(":", "\\:").replace("'", "\\'")


def build_filter(events):
    parts = []
    for ev in events:
        start = float(ev["t"])
        end = start + float(ev["duration"])
        text = escape_drawtext(ev["text"])
        parts.append(
            "drawtext="
            f"text='{text}':"
            "fontcolor=yellow:fontsize=36:borderw=2:bordercolor=black:"
            "x=40:y=h-th-40:"
            f"enable='between(t,{start},{end})'"
        )
    return ",".join(parts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--events", required=True, help="events.jsonl from session.sh's log_event")
    parser.add_argument("--input", required=True, help="raw screen-capture .mp4 from start_capture")
    parser.add_argument("--output", required=True, help="captioned .mp4 to write")
    args = parser.parse_args()

    events = []
    with open(args.events) as f:
        for line in f:
            line = line.strip()
            if line:
                events.append(json.loads(line))

    cmd = ["ffmpeg", "-y", "-i", args.input]
    if events:
        cmd += ["-vf", build_filter(events)]
    cmd += ["-c:a", "copy", args.output]

    print("+", " ".join(cmd), file=sys.stderr)
    subprocess.run(cmd, check=True)


if __name__ == "__main__":
    main()
