#!/usr/bin/env python

# This script commands a running Strand Camera instance to reset its
# background model, exactly as the buttons in the browser UI do:
#
# - By default, it presses "Take Current Image As Background", which
#   re-initializes the background model from the next ~20 incoming frames.
# - With `--clear-to-value N`, it instead presses the equivalent of "Set
#   background to mid-gray", setting the background mean to the uniform
#   gray value N (0-255) with zero variance. The browser button uses 127.

import argparse
import os
import json
import urllib
import requests  # https://docs.python-requests.org/en/latest/user/install

COOKIE_JAR_FNAME = "strand-cam-cookies.json"


class StrandCamProxy:
    def __init__(self, strand_cam_url):
        if not strand_cam_url.endswith("/"):
            strand_cam_url = strand_cam_url + "/"
        self.callback_url = urllib.parse.urljoin(strand_cam_url, "callback")

        self.session = requests.session()
        # If we have a cookie jar, load the cookies before initial request. This
        # allows using a URL without a token.
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

    def send_callback(self, payload):
        r = self.session.post(self.callback_url, json=payload)
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
        "--clear-to-value",
        type=float,
        default=None,
        metavar="N",
        help="instead of taking a new background image, set the background "
        "to the uniform gray value N (0-255; the browser UI uses 127)",
    )
    args = parser.parse_args()
    strand_cam = StrandCamProxy(strand_cam_url=args.strand_cam_url)

    if args.clear_to_value is None:
        strand_cam.send_callback("TakeCurrentImageAsBackground")
        print("Re-initializing background model from incoming images.")
    else:
        strand_cam.send_callback({"ClearBackground": args.clear_to_value})
        print("Set background model to uniform value %s." % args.clear_to_value)


if __name__ == "__main__":
    main()
