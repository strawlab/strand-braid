#!/usr/bin/env python

# This script commands a running Strand Camera instance to record a short MP4
# using the `Ffmpeg` codec, which pipes frames to the system `ffmpeg` binary.
# This is the same as choosing an "ffmpeg" codec in the browser UI and pressing
# "Record".
#
# Unlike the built-in H.264 encoders, the ffmpeg codec accepts color
# (e.g. `RGB8`) input, so this is the path to use for color recordings on
# cameras or GPUs where the built-in NVENC encoder is unavailable. Launch
# Strand Camera with `--pixel-format RGB8` (or configure the pixel format in the
# browser) for a color recording.
#
# With `--verify-dir DIR`, the script additionally checks that a non-empty
# `.mp4` file appeared in DIR (the `--data-dir` passed to strand-cam), which
# makes this usable as an end-to-end recording test. See the RGB8 phase of
# `smoke-tests/braid-camemu.sh`.

import argparse
import glob
import json
import os
import sys
import threading
import time
import urllib
import requests  # https://docs.python-requests.org/en/latest/user/install

COOKIE_JAR_FNAME = "strand-cam-cookies.json"


def parse_chunk(chunk):
    lines = chunk.strip().split(b"\n")
    if len(lines) != 2 or lines[0] != b"event: strand-cam":
        return None
    data_prefix = b"data: "
    assert lines[1].startswith(data_prefix)
    return json.loads(lines[1][len(data_prefix):])


def maintain_state_copy(event_iterator, shared_state):
    for chunk in event_iterator:
        data = parse_chunk(chunk)
        if data is not None:
            shared_state.update(data)


class StrandCamProxy:
    def __init__(self, strand_cam_url):
        if not strand_cam_url.endswith("/"):
            strand_cam_url = strand_cam_url + "/"
        self.callback_url = urllib.parse.urljoin(strand_cam_url, "callback")

        self.session = requests.session()
        # If we have a cookie jar, load the cookies before the initial request.
        # This allows using a URL without a token.
        if os.path.isfile(COOKIE_JAR_FNAME):
            with open(COOKIE_JAR_FNAME, "r") as f:
                cookies = requests.utils.cookiejar_from_dict(json.load(f))
                self.session.cookies.update(cookies)

        # Pass any token given and setup cookies.
        r = self.session.get(strand_cam_url)
        r.raise_for_status()

        # Store cookies
        with open(COOKIE_JAR_FNAME, "w") as f:
            json.dump(requests.utils.dict_from_cookiejar(self.session.cookies), f)

        # Create an iterator which is updated with each new event, and mirror
        # the camera state into `self.shared_state` on a background thread.
        events_url = urllib.parse.urljoin(strand_cam_url, "strand-cam-events")
        r = self.session.get(
            events_url, stream=True, headers={"Accept": "text/event-stream"},
        )
        r.raise_for_status()
        event_iterator = r.iter_content(chunk_size=None)

        self.shared_state = {}
        thread = threading.Thread(
            target=maintain_state_copy, args=(event_iterator, self.shared_state)
        )
        thread.daemon = True
        thread.start()

    def wait_until_first_update(self):
        while len(self.shared_state.keys()) == 0:
            time.sleep(0.1)

    def send_to_camera(self, cmd_dict):
        r = self.session.post(self.callback_url, json={"ToCamera": cmd_dict})
        r.raise_for_status()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--strand-cam-url",
        type=str,
        default="http://127.0.0.1:3440/",
        help="URL of Strand Camera",
    )
    parser.add_argument(
        "--codec",
        type=str,
        default="libx264",
        help="ffmpeg codec to encode with (e.g. libx264, h264_nvenc, "
        "h264_vaapi). Must be available in the system ffmpeg. Default: libx264",
    )
    parser.add_argument(
        "--duration",
        type=float,
        default=3.0,
        help="number of seconds to record",
    )
    parser.add_argument(
        "--verify-dir",
        type=str,
        default=None,
        metavar="DIR",
        help="after recording, verify that a non-empty .mp4 was written to DIR "
        "(the --data-dir passed to strand-cam). Exits non-zero on failure.",
    )
    args = parser.parse_args()

    if args.verify_dir is not None:
        preexisting = set(glob.glob(os.path.join(args.verify_dir, "*.mp4")))

    strand_cam = StrandCamProxy(strand_cam_url=args.strand_cam_url)
    strand_cam.wait_until_first_update()

    # Select the ffmpeg codec. `SetMp4Codec` takes a `CodecSelection`; the
    # `Ffmpeg` variant carries the ffmpeg arguments (here just the codec name).
    strand_cam.send_to_camera({"SetMp4Codec": {"Ffmpeg": {"codec": args.codec}}})

    strand_cam.send_to_camera({"SetIsRecordingMp4": True})
    print("Recording for %g seconds with ffmpeg codec %r..." % (args.duration, args.codec))
    time.sleep(args.duration)

    strand_cam.send_to_camera({"SetIsRecordingMp4": False})
    print("...finished.")

    if args.verify_dir is not None:
        verify_new_mp4(args.verify_dir, preexisting)


def verify_new_mp4(verify_dir, preexisting, min_bytes=1024, timeout=30.0):
    # After recording stops, strand-cam's writer thread finalizes the MP4
    # (ffmpeg must flush and write the moov atom), so poll until a new,
    # non-empty file settles.
    deadline = time.time() + timeout
    while time.time() < deadline:
        new_files = set(glob.glob(os.path.join(verify_dir, "*.mp4"))) - preexisting
        ready = [f for f in new_files if os.path.getsize(f) >= min_bytes]
        if ready:
            for f in ready:
                print("Verified MP4: %s (%d bytes)" % (f, os.path.getsize(f)))
            return
        time.sleep(0.5)
    sys.exit(
        "FAILED: no non-empty .mp4 (>= %d bytes) appeared in %s within %g s"
        % (min_bytes, verify_dir, timeout)
    )


if __name__ == "__main__":
    main()
