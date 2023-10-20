#!/usr/bin/env python
from __future__ import print_function
import argparse
import json
import time
import threading
import sys
import requests  # https://docs.python-requests.org/en/latest/user/install


def maintain_state_copy(event_iterator, shared_state):
    for chunk in event_iterator:
        data = parse_chunk(chunk)
        if data is not None:
            shared_state.update(data)


def parse_chunk(chunk):
    lines = chunk.strip().split(b"\n")
    assert len(lines) == 2
    if lines[0] != b"event: strand-cam":
        return None

    strand_cam_message = lines[1]
    data_prefix = b"data: "
    assert strand_cam_message.startswith(data_prefix)
    buf = strand_cam_message[len(data_prefix) :]
    data = json.loads(buf)
    return data


class StrandCamProxy:
    def __init__(self, strand_cam_url):
        if not strand_cam_url.endswith("/"):
            strand_cam_url = strand_cam_url + "/"
        self.callback_url = strand_cam_url + "callback"

        # Setup initial session
        self.session = requests.session()
        r = self.session.get(strand_cam_url)
        assert r.status_code == requests.codes.ok

        # Create iterator which is updated with each new event
        events_url = strand_cam_url + "strand-cam-events"
        r = self.session.get(
            events_url, stream=True, headers={"Accept": "text/event-stream"},
        )
        assert r.status_code == requests.codes.ok
        event_iterator = r.iter_content(chunk_size=None)

        # Send this iterator to a new thread
        self.shared_state = {}
        thread = threading.Thread(
            target=maintain_state_copy, args=(event_iterator, self.shared_state)
        )
        thread.setDaemon(True)
        thread.start()

    def get_current_state(self):
        return self.shared_state

    def wait_until_first_update(self):
        while len(self.shared_state.keys()) == 0:
            time.sleep(0.1)

    def send_to_camera(self, cmd_dict):
        params = {"ToCamera": cmd_dict}
        body = json.dumps(params)
        r = self.session.post(self.callback_url, data=body)
        if r.status_code != requests.codes.ok:
            print(
                "error making request, status code {}".format(r.status_code),
                file=sys.stderr,
            )
            sys.exit(1)


def main():
    parser = argparse.ArgumentParser()

    parser.add_argument(
        "--strand-cam-url",
        type=str,
        default="http://127.0.0.1:3440/",
        help="URL of Strand Camera",
    )
    args = parser.parse_args()
    strand_cam = StrandCamProxy(strand_cam_url=args.strand_cam_url)
    strand_cam.wait_until_first_update()

    strand_cam.send_to_camera({"SetIsRecordingMp4": True})
    print("Recording for 5 seconds...")
    time.sleep(5.0)

    strand_cam.send_to_camera({"SetIsRecordingMp4": False})
    print("...finished.")


if __name__ == "__main__":
    main()
