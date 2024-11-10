#!/usr/bin/env python
import argparse
import os
import json
import time
import threading
import urllib
import requests  # https://docs.python-requests.org/en/latest/user/install

COOKIE_JAR_FNAME = "strand-cam-cookies.json"

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
        self.callback_url = urllib.parse.urljoin(strand_cam_url, "callback")

        self.session = requests.session()
        # If we have a cookie jar, load the cookies before initial request. This
        # allows using a URL without a token.
        if os.path.isfile(COOKIE_JAR_FNAME):
            with open(COOKIE_JAR_FNAME, 'r') as f:
                cookies = requests.utils.cookiejar_from_dict(json.load(f))
                self.session.cookies.update(cookies)

        # Pass any token given and setup cookies.
        r = self.session.get(strand_cam_url)
        r.raise_for_status()

        # Store cookies
        with open(COOKIE_JAR_FNAME, 'w') as f:
            json.dump(requests.utils.dict_from_cookiejar(self.session.cookies), f)

        # Create iterator which is updated with each new event
        events_url = urllib.parse.urljoin(strand_cam_url, "strand-cam-events")
        r = self.session.get(
            events_url, stream=True, headers={"Accept": "text/event-stream"},
        )
        r.raise_for_status()
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
        r = self.session.post(self.callback_url, json=params)
        r.raise_for_status()


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
