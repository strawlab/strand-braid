#!/usr/bin/env python
from __future__ import print_function
import argparse
import requests
import json
import time
import threading

DATA_PREFIX = "data: "


def maintain_state_copy(event_iterator, shared_state):
    for chunk in event_iterator:
        data = parse_chunk(chunk)
        if data is not None:
            shared_state.update(data)


def parse_chunk(chunk):
    lines = chunk.strip().split("\n")
    assert len(lines) == 2
    if lines[0] != "event: bui_backend":
        return None
    assert lines[1].startswith(DATA_PREFIX)
    buf = lines[1][len(DATA_PREFIX) :]
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

    def get_led1_state(self):
        return self.shared_state["camtrig_device_state"]["ch1"]

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
        body = json.dumps(params)
        r = self.session.post(self.callback_url, data=body)
        if r.status_code != requests.codes.ok:
            print(
                "error making request, status code {}".format(r.status_code),
                file=sys.stderr,
            )
            sys.exit(1)
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
    strand_cam = StrandCamProxy(strand_cam_url=args.strand_cam_url)
    strand_cam.wait_until_first_update()
    while 1:
        strand_cam.send_config(mode="Off")
        print("current LED state: ", strand_cam.get_led1_state())
        time.sleep(5.0)
        strand_cam.send_config(mode="PositionTriggered")
        print("current LED state: ", strand_cam.get_led1_state())
        time.sleep(5.0)


if __name__ == "__main__":
    main()
