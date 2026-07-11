# /// script
# requires-python = ">=3.9"
# dependencies = ["requests"]
# ///
"""Capture Braid model-server SSE events for a fixed duration.

Usage: sse_capture.py <url> <duration_s> <out_csv>

Writes one row per event: recv_time (local, s since epoch), latency (s, as
reported by the model server: publish time minus trigger timestamp),
synced_frame, msg_type.
"""
import csv
import json
import sys
import time

import requests

url, duration, out_csv = sys.argv[1], float(sys.argv[2]), sys.argv[3]
deadline = time.time() + duration

rows = []
try:
    with requests.get(url, stream=True, timeout=(5, duration + 10),
                      headers={"Accept": "text/event-stream"}) as r:
        r.raise_for_status()
        for line in r.iter_lines(decode_unicode=True):
            if time.time() > deadline:
                break
            if not line or not line.startswith("data: "):
                continue
            recv = time.time()
            try:
                d = json.loads(line[len("data: "):])
            except json.JSONDecodeError:
                continue
            msg = d.get("msg")
            msg_type = next(iter(msg)) if isinstance(msg, dict) else str(msg)
            rows.append((recv, d.get("latency"), d.get("synced_frame"), msg_type))
except requests.exceptions.ReadTimeout:
    pass

with open(out_csv, "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["recv_time", "latency_s", "synced_frame", "msg_type"])
    w.writerows(rows)
print(f"captured {len(rows)} events -> {out_csv}")
