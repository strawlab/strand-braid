#!/usr/bin/env python
import argparse
import json
import requests  # https://docs.python-requests.org/en/latest/user/install
import urllib.request
import os

data_prefix = b"data: "
event_prefix = b"event: "
strand_cam_event = b"strand-cam"
image_event = b"http-video-streaming"
jpeg_prefix = "data:image/jpeg;"
cookie_jar_fname = "strand-cam-cookies.json"

def parse_json_event(chunk):
    lines = chunk.strip().split(b"\n")
    assert len(lines) == 2
    if not lines[0].startswith(event_prefix):
        return None, None

    event_type = lines[0][len(event_prefix):]
    if event_type not in [strand_cam_event, image_event]:
        return None, None
    strand_cam_message = lines[1]
    assert strand_cam_message.startswith(data_prefix)
    buf = strand_cam_message[len(data_prefix) :]
    data = json.loads(buf)

    return event_type, data

class StrandCamProxy:
    """Encapsulates interaction with Strand Camera

    Subscribes to the server sent events and facilitates making callbacks."""
    def __init__(self, strand_cam_url):
        # Setup initial session.
        self.session = requests.session()

        # If we have a cookie jar, load the cookies before initial request. This
        # allows using a URL without a token.
        if os.path.isfile(cookie_jar_fname):
            with open(cookie_jar_fname, 'r') as f:
                cookies = requests.utils.cookiejar_from_dict(json.load(f))
                self.session.cookies.update(cookies)

        # Pass any token given and setup cookies.
        r = self.session.get(strand_cam_url)
        r.raise_for_status()

        # Store cookies
        with open(cookie_jar_fname, 'w') as f:
            json.dump(requests.utils.dict_from_cookiejar(self.session.cookies), f)

        self.callback_url = urllib.parse.urljoin(strand_cam_url, "callback")

        # Create iterator which is updated with each new event
        events_url = urllib.parse.urljoin(strand_cam_url, "strand-cam-events")
        r = self.session.get(
            events_url, stream=True, headers={"Accept": "text/event-stream"},
        )
        r.raise_for_status()
        self.event_iterator = r.iter_content(chunk_size=None)

    def post(self, data):
        r = self.session.post(self.callback_url, json=data)
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

    # infinite loop where we wait for an event from Strand Camera
    for chunk in strand_cam.event_iterator:
        event_type, data = parse_json_event(chunk)
        # We are only interested in image events here.
        if event_type == image_event:
            # Extract the data URL into its raw binary bytes
            assert data["firehose_frame_data_url"].startswith(jpeg_prefix)
            response = urllib.request.urlopen(data["firehose_frame_data_url"])
            bytes = response.file.read()

            # Dump the bytes to a jpeg file.
            fname = "image%04d.jpg"%data["fno"]
            with open(fname, mode="wb") as fd:
                fd.write(bytes)
            print(f"saved {fname}")
            fd.close()

            # Send "we received the image and are thus ready for a new one".
            strand_cam.post({"FirehoseNotify": data["ck"]})

if __name__ == "__main__":
    main()
