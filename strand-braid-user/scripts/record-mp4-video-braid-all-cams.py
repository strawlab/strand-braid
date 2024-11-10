#!/usr/bin/env python
import argparse
import json
import time
import sys
import urllib
import requests  # https://docs.python-requests.org/en/latest/user/install
import os

COOKIE_JAR_FNAME = "braid-cookies.json"


class BraidProxy:
    def __init__(self, braid_url):
        self.callback_url = urllib.parse.urljoin(braid_url, "callback")
        # Setup initial session
        self.session = requests.session()

        # If we have a cookie jar, load the cookies before initial request. This
        # allows using a URL without a token.
        if os.path.isfile(COOKIE_JAR_FNAME):
            with open(COOKIE_JAR_FNAME, 'r') as f:
                cookies = requests.utils.cookiejar_from_dict(json.load(f))
                self.session.cookies.update(cookies)

        r = self.session.get(braid_url)
        r.raise_for_status()

        # Store cookies
        with open(COOKIE_JAR_FNAME, 'w') as f:
            json.dump(requests.utils.dict_from_cookiejar(self.session.cookies), f)


    def send(self, cmd_dict):
        r = self.session.post(
            self.callback_url, json=cmd_dict}
        )
        r.raise_for_status()


def main():
    parser = argparse.ArgumentParser()

    parser.add_argument(
        "--braid-url",
        type=str,
        default="http://127.0.0.1:33333/",
        help="URL of Braid",
    )
    args = parser.parse_args()
    braid = BraidProxy(braid_url=args.braid_url)

    braid.send({"DoRecordMp4Files": True})
    print("Recording for 5 seconds...")
    time.sleep(5.0)

    braid.send({"DoRecordMp4Files": False})
    print("...finished.")


if __name__ == "__main__":
    main()
