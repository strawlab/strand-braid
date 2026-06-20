#!/usr/bin/env python

# This script connects to a running Braid instance, discovers all connected
# Strand Camera instances, and commands each of them to reset its background
# model, exactly as the buttons in the browser UI of each camera do:
#
# - By default, it presses "Take Current Image As Background" on every
#   camera, re-initializing each background model from the next ~20 incoming
#   frames.
# - With `--clear-to-value N`, it instead presses the equivalent of "Set
#   background to mid-gray" on every camera, setting each background mean to
#   the uniform gray value N (0-255). The browser button uses 127.
#
# There is no single Braid command for this; instead, Braid is asked for the
# HTTP address of every connected camera and each camera is commanded
# directly. The same pattern works for any other per-camera action.

import argparse
import json
import os
import urllib
import requests  # https://docs.python-requests.org/en/latest/user/install

DATA_PREFIX = "data: "
COOKIE_JAR_FNAME = "braid-cookies.json"


class BraidProxy:
    def __init__(self, braid_url):
        if not braid_url.endswith("/"):
            braid_url = braid_url + "/"
        self.braid_url = braid_url
        self.session = requests.session()

        # If we have a cookie jar, load the cookies before initial request. This
        # allows using a URL without a token.
        if os.path.isfile(COOKIE_JAR_FNAME):
            with open(COOKIE_JAR_FNAME, "r") as f:
                cookies = requests.utils.cookiejar_from_dict(json.load(f))
                self.session.cookies.update(cookies)

        r = self.session.get(self.braid_url)
        r.raise_for_status()

        # Store cookies
        with open(COOKIE_JAR_FNAME, "w") as f:
            json.dump(requests.utils.dict_from_cookiejar(self.session.cookies), f)

    def get_connected_cameras(self):
        """Read Braid's state and return the connected camera list."""
        events_url = urllib.parse.urljoin(self.braid_url, "braid-events")
        r = self.session.get(
            events_url, stream=True, headers={"Accept": "text/event-stream"},
        )
        r.raise_for_status()
        # The first event carries a full copy of Braid's current state.
        for chunk in r.iter_content(chunk_size=None, decode_unicode=True):
            state = parse_chunk(chunk)
            r.close()
            return state["connected_cameras"]
        raise RuntimeError("no event received from Braid")


def parse_chunk(chunk):
    lines = chunk.strip().split("\n")
    assert len(lines) == 2
    assert lines[0] == "event: braid"
    assert lines[1].startswith(DATA_PREFIX)
    buf = lines[1][len(DATA_PREFIX):]
    return json.loads(buf)


def strand_cam_url(cam_info, braid_url):
    """Build the URL (including any access token) of one camera, or None."""
    server_info = cam_info["strand_cam_http_server_info"]
    if server_info == "NoServer":
        return None
    addr = server_info["Server"]["addr"]
    token = server_info["Server"]["token"]
    # If the camera listens on an unspecified address, assume it is reachable
    # on the same host as Braid.
    host_port = addr.rsplit(":", 1)
    if host_port[0] in ("0.0.0.0", "[::]"):
        braid_host = urllib.parse.urlparse(braid_url).hostname
        addr = "%s:%s" % (braid_host, host_port[1])
    url = "http://%s/" % addr
    if token != "NoToken":
        url = url + "?token=" + token["PreSharedToken"]
    return url


def send_callback(cam_url, payload):
    session = requests.session()
    # Pass any token given and setup cookies.
    r = session.get(cam_url)
    r.raise_for_status()
    callback_url = urllib.parse.urljoin(cam_url, "callback")
    r = session.post(callback_url, json=payload)
    r.raise_for_status()


def main():
    parser = argparse.ArgumentParser()

    parser.add_argument(
        "--braid-url",
        type=str,
        default="http://127.0.0.1:8397/",
        help="URL of Braid",
    )
    parser.add_argument(
        "--clear-to-value",
        type=float,
        default=None,
        metavar="N",
        help="instead of taking a new background image, set the background "
        "to the uniform gray value N (0-255; the browser UI uses 127)",
    )
    args = parser.parse_args()

    if args.clear_to_value is None:
        payload = "TakeCurrentImageAsBackground"
    else:
        payload = {"ClearBackground": args.clear_to_value}

    braid = BraidProxy(braid_url=args.braid_url)
    for cam_info in braid.get_connected_cameras():
        name = cam_info["name"]
        cam_url = strand_cam_url(cam_info, args.braid_url)
        if cam_url is None:
            print("%s: no HTTP server, skipping" % name)
            continue
        send_callback(cam_url, payload)
        print("%s: sent %s" % (name, json.dumps(payload)))


if __name__ == "__main__":
    main()
