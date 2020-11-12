#!/usr/bin/env python

# This script listens to the HTTP JSON Event Stream of Braid and
# transmits pose information over UDP in a simple text format.

# This is an example of listening for the live stream of Braid. In version 1 of
# the Braid pose API, in addition to the `Update` events, Braid also has `Birth`
# and `Death` events. The `Birth` event returns the same data as an `Update`
# event, whereas the `Death` event sends just `obj_id`. Version 2 added the
# EndOfFrame message type.

from __future__ import print_function
import argparse
import requests
import json
import time
import socket

DATA_PREFIX = "data: "


class BraidProxy:
    def __init__(self, braid_model_server_url):
        self.braid_model_server_url = braid_model_server_url
        self.session = requests.session()
        r = self.session.get(self.braid_model_server_url)
        assert r.status_code == requests.codes.ok

    def run(self, udp_host, udp_port):
        addr = (udp_host, udp_port)
        print("sending braid pose data to UDP %s" % (addr,))
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        events_url = self.braid_model_server_url + "events"
        r = self.session.get(
            events_url, stream=True, headers={"Accept": "text/event-stream"},
        )
        for chunk in r.iter_content(chunk_size=None, decode_unicode=True):
            data = parse_chunk(chunk)
            # print('chunk value: %r'%data)
            version = data.get("v", 1)  # default because missing in first release
            assert version in (1, 2)  # check the data version

            try:
                update_dict = data["Update"]
            except KeyError:
                continue
            msg = "%s, %s, %s" % (update_dict["x"], update_dict["y"], update_dict["z"])
            sock.sendto(msg, addr)
            # print('send message %r to %s'%(msg,addr))


def parse_chunk(chunk):
    lines = chunk.strip().split("\n")
    assert len(lines) == 2
    assert lines[0] == "event: braid-pose"
    assert lines[1].startswith(DATA_PREFIX)
    buf = lines[1][len(DATA_PREFIX) :]
    data = json.loads(buf)
    return data


def main():
    parser = argparse.ArgumentParser()

    parser.add_argument(
        "--braid-model-server-url",
        default="http://127.0.0.1:8397/",
        help="URL of Braid model server",
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
    BraidProxy(args.braid_model_server_url).run(
        udp_host=args.udp_host, udp_port=args.udp_port
    )


if __name__ == "__main__":
    main()
