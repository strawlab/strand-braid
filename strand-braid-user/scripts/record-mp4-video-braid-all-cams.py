#!/usr/bin/env python
import argparse
import json
import time
import sys
from urllib.parse import urlparse
import requests  # https://docs.python-requests.org/en/latest/user/install


class BraidProxy:
    def __init__(self, braid_url):
        self.callback_url = urlparse(braid_url)._replace(path="callback").geturl()
        # Setup initial session
        self.session = requests.session()
        r = self.session.get(braid_url)
        if r.status_code != requests.codes.ok:
            print(f"request URL: {braid_url}")
            print("request failed. response:")
            print(r.text)
            raise RuntimeError("connection to braid failed.")

    def send(self, cmd_dict):
        body = json.dumps(cmd_dict)
        r = self.session.post(
            self.callback_url, data=body, headers={"Content-Type": "application/json"}
        )
        if r.status_code != requests.codes.ok:
            print(
                "error making request, status code {}".format(r.status_code),
                file=sys.stderr,
            )
            sys.exit(1)


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
