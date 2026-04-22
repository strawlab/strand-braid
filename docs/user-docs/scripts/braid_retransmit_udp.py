#!/usr/bin/env python

# This script listens to the HTTP JSON Event Stream of braid and transmits
# pose information over UDP in a simple text format.

# This is an example of listening for the live stream of braid. There are 3
# event types. In addition to the `Update` events, there are also `Birth` and
# `Death` events. The `Birth` event returns the same data as an `Update` event,
# whereas the `Death` event sends just `obj_id`.

import argparse
import requests
import json
import socket
import os

DATA_PREFIX = "data: "
COOKIE_JAR_FNAME = "braid-cookies.json"

class BraidProxy:
    def __init__(self, braid_url):
        self.braid_url = braid_url
        self.session = requests.session()

        # If we have a cookie jar, load the cookies before initial request. This
        # allows using a URL without a token.
        if os.path.isfile(COOKIE_JAR_FNAME):
            with open(COOKIE_JAR_FNAME, 'r') as f:
                cookies = requests.utils.cookiejar_from_dict(json.load(f))
                self.session.cookies.update(cookies)

        r = self.session.get(self.braid_url)
        r.raise_for_status()

        # Store cookies
        with open(COOKIE_JAR_FNAME, 'w') as f:
            json.dump(requests.utils.dict_from_cookiejar(self.session.cookies), f)


    def run(self, udp_host, udp_port):
        addr = (udp_host, udp_port)
        print("sending flydra data to UDP %s" % (addr,))
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        events_url = self.braid_url + "events"
        r = self.session.get(
            events_url, stream=True, headers={"Accept": "text/event-stream"},
        )
        for chunk in r.iter_content(chunk_size=None, decode_unicode=True):
            data = parse_chunk(chunk)
            # print('chunk value: %r'%data)
            version = data.get("v", 1)  # default because missing in first release
            assert version in (2, 3)  # check the data version

            try:
                update_dict = data["msg"]["Update"]
            except KeyError:
                continue
            msg = "%s, %s, %s" % (update_dict["x"], update_dict["y"], update_dict["z"])
            msg = msg.encode('ascii')
            sock.sendto(msg, addr)
            # print('send message %r to %s'%(msg,addr))


def parse_chunk(chunk):
    lines = chunk.strip().split("\n")
    assert len(lines) == 2
    assert lines[0] == "event: braid"
    assert lines[1].startswith(DATA_PREFIX)
    buf = lines[1][len(DATA_PREFIX) :]
    data = json.loads(buf)
    return data


def main():
    parser = argparse.ArgumentParser()

    parser.add_argument(
        "--braid-url", default="http://127.0.0.1:8397/", help="URL of braid server"
    )

    parser.add_argument(
        "--udp-port", type=int, default=1234, help="UDP port to send pose information"
    )
    parser.add_argument(
        "--udp-host",
        type=str,
        default="127.0.0.1",
        help="UDP host to send pose information",
    )
    args = parser.parse_args()
    braid = BraidProxy(args.braid_url)
    braid.run(udp_host=args.udp_host, udp_port=args.udp_port)


if __name__ == "__main__":
    main()
