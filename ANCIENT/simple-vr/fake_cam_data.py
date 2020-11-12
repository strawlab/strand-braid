import math
import json
import socket
import time

UDP_IP = "127.0.0.1"
UDP_PORT = 3443
sock = socket.socket(socket.AF_INET,  # Internet
                     socket.SOCK_DGRAM)  # UDP

while 1:
    now = time.time()
    t = now % 1.0
    theta = 2*math.pi*t
    x = (math.cos(theta) * 100) + 320
    y = (math.sin(theta) * 100) + 240

    t_sec = int(now)
    t_nsec = int(t*1e9)

    timestamp = {
            "sec": t_sec,
            "nsec": t_nsec
      }
    if t < 0.06:
        # No valid point this frame
        feature = None
    else:
        # Found a valid point
        feature = {'pixel_xy': [x, y], 'theta': 0.123}
    msg = {"timestamp": timestamp, "feature": feature}
    buf = json.dumps(msg)
    sock.sendto(buf, (UDP_IP, UDP_PORT))
    time.sleep(0.05)
