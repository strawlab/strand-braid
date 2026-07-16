#!/usr/bin/env python3

# Finds the on-screen bounding box of a piece of DOM text in a Chrome/
# Chromium tab via the Chrome DevTools Protocol (CDP), so callers can point
# the mouse at real UI text (e.g. a camera name) instead of guessing tuned
# pixel offsets that break whenever the page layout changes.
#
# No third-party dependencies -- run with plain python3. CDP's
# Runtime.evaluate needs a WebSocket connection, and no WebSocket library is
# assumed to be installed, so this hand-rolls the minimal client needed for
# one request/response exchange (RFC 6455): a client handshake, one masked
# text frame out, one (possibly fragmented) text frame back.
#
# Usage:
#   python3 cdp_locate.py --port 9333 --contains "Live view - "
#
# Prints a JSON object on stdout: {"x":.., "y":.., "width":.., "height":..,
# "chromeY":..} where x/y/width/height are the CSS-pixel bounding box (in
# page/viewport coordinates) of the smallest element whose text contains
# the given substring, and chromeY is the browser's own chrome height
# (window.outerHeight - window.innerHeight) needed to convert a viewport
# coordinate into a window-relative one. Exits non-zero with a message on
# stderr if the tab can't be reached or no matching element is found.

import argparse
import base64
import hashlib
import json
import os
import socket
import struct
import sys
import urllib.request


def find_page_ws_url(port):
    with urllib.request.urlopen(f"http://127.0.0.1:{port}/json", timeout=5) as f:
        targets = json.load(f)
    for t in targets:
        if t.get("type") == "page" and "webSocketDebuggerUrl" in t:
            return t["webSocketDebuggerUrl"]
    raise RuntimeError("no page target found on CDP port")


def ws_handshake(sock, host, port, path):
    key = base64.b64encode(os.urandom(16)).decode()
    req = (
        f"GET {path} HTTP/1.1\r\n"
        f"Host: {host}:{port}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        "Sec-WebSocket-Version: 13\r\n"
        "\r\n"
    )
    sock.sendall(req.encode())
    resp = b""
    while b"\r\n\r\n" not in resp:
        chunk = sock.recv(4096)
        if not chunk:
            raise RuntimeError("connection closed during WebSocket handshake")
        resp += chunk
    if b"101" not in resp.split(b"\r\n", 1)[0]:
        raise RuntimeError(f"WebSocket handshake failed: {resp[:200]!r}")
    expected = base64.b64encode(
        hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()
    ).decode()
    if expected.encode() not in resp:
        raise RuntimeError("WebSocket handshake Sec-WebSocket-Accept mismatch")


def ws_send_text(sock, payload):
    data = payload.encode()
    mask = os.urandom(4)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
    length = len(masked)
    if length < 126:
        header = struct.pack("!BB", 0x81, 0x80 | length)
    elif length < 65536:
        header = struct.pack("!BBH", 0x81, 0x80 | 126, length)
    else:
        header = struct.pack("!BBQ", 0x81, 0x80 | 127, length)
    sock.sendall(header + mask + masked)


def _recv_exact(sock, n):
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise RuntimeError("connection closed while reading WebSocket frame")
        buf += chunk
    return buf


def ws_recv_text(sock):
    # Loops past control frames (ping/pong/close) and reassembles a
    # fragmented text message, since a real reply could arrive as more than
    # one frame -- we only need to handle the server->client (unmasked)
    # direction here.
    message = b""
    while True:
        header = _recv_exact(sock, 2)
        b0, b1 = header[0], header[1]
        fin = b0 & 0x80
        opcode = b0 & 0x0F
        length = b1 & 0x7F
        if length == 126:
            length = struct.unpack("!H", _recv_exact(sock, 2))[0]
        elif length == 127:
            length = struct.unpack("!Q", _recv_exact(sock, 8))[0]
        payload = _recv_exact(sock, length) if length else b""
        if opcode == 0x8:  # close
            raise RuntimeError("WebSocket closed by server")
        if opcode in (0x9, 0xA):  # ping/pong: not relevant for one request/response
            continue
        message += payload
        if fin:
            return message.decode()


def cdp_evaluate(port, expression):
    ws_url = find_page_ws_url(port)
    # ws://host:port/path
    rest = ws_url.split("://", 1)[1]
    hostport, _, path = rest.partition("/")
    path = "/" + path
    host, _, ws_port = hostport.partition(":")
    ws_port = int(ws_port) if ws_port else 80

    sock = socket.create_connection((host, ws_port), timeout=5)
    try:
        ws_handshake(sock, host, ws_port, path)
        msg = {"id": 1, "method": "Runtime.evaluate", "params": {"expression": expression, "returnByValue": True}}
        ws_send_text(sock, json.dumps(msg))
        reply = json.loads(ws_recv_text(sock))
    finally:
        sock.close()

    result = reply.get("result", {})
    if "exceptionDetails" in result:
        raise RuntimeError(f"JS evaluation error: {result['exceptionDetails']}")
    return result.get("result", {}).get("value")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", required=True, type=int, help="Chrome --remote-debugging-port")
    parser.add_argument("--contains", required=True, help="substring to search for in element text")
    args = parser.parse_args()

    needle = json.dumps(args.contains)
    expression = (
        "(function(){"
        "var all=document.querySelectorAll('*');"
        "var best=null,bestArea=Infinity;"
        "for(var i=0;i<all.length;i++){"
        "var el=all[i];"
        f"if(el.textContent&&el.textContent.indexOf({needle})!==-1){{"
        "var r=el.getBoundingClientRect();"
        "var area=r.width*r.height;"
        "if(area>0&&area<bestArea){bestArea=area;best=r;}"
        "}}"
        "if(!best)return null;"
        "return {x:best.x,y:best.y,width:best.width,height:best.height,"
        "chromeY:window.outerHeight-window.innerHeight};"
        "})()"
    )

    try:
        value = cdp_evaluate(args.port, expression)
    except Exception as e:
        print(f"ERROR: {e}", file=sys.stderr)
        sys.exit(1)

    if value is None:
        print(f"ERROR: no element found containing {args.contains!r}", file=sys.stderr)
        sys.exit(1)

    print(json.dumps(value))


if __name__ == "__main__":
    main()
