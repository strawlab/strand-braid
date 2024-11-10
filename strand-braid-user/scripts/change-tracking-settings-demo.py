#!/usr/bin/env python
import argparse
import requests
import json
import time
import threading
import urllib
import os

DATA_PREFIX = b"data: "
COOKIE_JAR_FNAME = "strand-cam-cookies.json"

def maintain_state_copy(event_iterator, shared_state):
    for chunk in event_iterator:
        data = parse_chunk(chunk)
        if data is not None:
            shared_state.update(data)


def parse_chunk(chunk):
    lines = chunk.strip().split(b"\n")
    print(lines)
    assert len(lines) == 2
    if lines[0] != b"event: strand-cam":
        return None
    assert lines[1].startswith(DATA_PREFIX)
    buf = lines[1][len(DATA_PREFIX) :]
    data = json.loads(buf)
    return data


class StrandCamProxy:
    def __init__(self, strand_cam_url):
        if not strand_cam_url.endswith("/"):
            strand_cam_url = strand_cam_url + "/"
        self.callback_url = urllib.parse.urljoin(strand_cam_url, "callback")

        self.session = requests.session()
        # If we have a cookie jar, load the cookies before initial request. This
        # allows using a URL without a token.
        if os.path.isfile(COOKIE_JAR_FNAME):
            with open(COOKIE_JAR_FNAME, 'r') as f:
                cookies = requests.utils.cookiejar_from_dict(json.load(f))
                self.session.cookies.update(cookies)

        # Setup initial session
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
        thread.daemon = True
        thread.start()

    def get_led1_state(self):
        led_box_device_state = self.shared_state["led_box_device_state"]
        if led_box_device_state is None:
            raise RuntimeError("Strand Cam does not include LED state, "
                               "are you using the flydratrax variant?")
        return led_box_device_state["ch1"]

    def wait_until_first_update(self):
        while len(self.shared_state.keys()) == 0:
            time.sleep(0.1)

    def send_config(self, mode):
        assert mode in ["Off", "PositionTriggered", "TwoStagePositionTriggered"]
        # yaml string
        CamArgSetLedProgramConfig = """---
led_trigger_mode: "{mode}"
led_on_shape_pixels:
  Circle:
    center_x: 640
    center_y: 512
    radius: 50
led_channel_num: 1
led_second_stage_radius: 50
led_hysteresis_pixels: 3.0
""".format(
            mode=mode
        )

        params = {"ToCamera": {"CamArgSetLedProgramConfig": CamArgSetLedProgramConfig}}
        r = self.session.post(self.callback_url, json=params)
        r.raise_for_status()
        print("made request with mode {mode}".format(mode=mode))


def main():
    parser = argparse.ArgumentParser()

    parser.add_argument(
        "--strand-cam-url",
        type=str,
        default="http://127.0.0.1:3440/",
        help="URL of Strand Camera",
    )
    args = parser.parse_args()
    print("1")
    strand_cam = StrandCamProxy(strand_cam_url=args.strand_cam_url)
    print("2")
    strand_cam.wait_until_first_update()
    print("3")
    while 1:
        print("4")
        strand_cam.send_config(mode="Off")
        print("current LED state: ", strand_cam.get_led1_state())
        time.sleep(5.0)
        strand_cam.send_config(mode="PositionTriggered")
        print("current LED state: ", strand_cam.get_led1_state())
        time.sleep(5.0)


if __name__ == "__main__":
    main()
