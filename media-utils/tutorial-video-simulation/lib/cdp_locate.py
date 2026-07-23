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
# page/viewport coordinates) of the substring itself (found in a single DOM
# text node and measured via a Range, not the enclosing element -- see
# below for why), and chromeY is the browser's own chrome height
# (window.outerHeight - window.innerHeight) needed to convert a viewport
# coordinate into a window-relative one. Exits non-zero with a message on
# stderr if the tab can't be reached or no matching text node is found.

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


def cdp_evaluate(port, expression, await_promise=False):
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
        # await_promise: needed when `expression` is an async IIFE (e.g. the
        # --click mode's mousedown/mouseup press animation below, which
        # awaits a real setTimeout between the two) -- without it, CDP
        # returns the Promise object itself instead of waiting for it to
        # resolve.
        msg = {
            "id": 1,
            "method": "Runtime.evaluate",
            "params": {"expression": expression, "returnByValue": True, "awaitPromise": await_promise},
        }
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
    parser.add_argument(
        "--get-href",
        action="store_true",
        help=(
            "instead of a bounding box, print {'href': ...} -- the resolved absolute URL of the "
            "nearest ancestor <a> of the matching text node. Used to read a real link's actual "
            "target (e.g. a same-origin app route whose exact URL-encoding isn't worth "
            "reimplementing in bash) rather than reconstructing it by hand."
        ),
    )
    parser.add_argument(
        "--click",
        action="store_true",
        help=(
            "instead of a bounding box, call .click() on the nearest ancestor of the matching "
            "text node matching --click-ancestor -- a real DOM click dispatched "
            "programmatically, so whatever listener the app registered (e.g. a Yew onclick "
            "callback) actually fires, the same way navigate_browser substitutes for a literal "
            "click on a link. Also overrides window.confirm/alert to auto-accept first, since a "
            "real click's handler may call window.confirm() synchronously -- that's a native, "
            "blocking dialog this hand-rolled client has no way to intercept otherwise (would "
            "hang waiting for a response that never comes)."
        ),
    )
    parser.add_argument(
        "--click-ancestor",
        default="button",
        help=(
            "element.closest() selector used by --click to find the clickable ancestor of the "
            "matching text node (default: button). Some widgets have no <button> in their DOM "
            "at all -- e.g. this project's <Toggle> component (web/ads-webasm/src/components/"
            "toggle.rs) renders a bare <label><input type=checkbox></label>, so toggling it "
            "needs --click-ancestor label: clicking a <label> natively activates its associated "
            "<input> per HTML's own label-click behavior, without needing to locate the <input> "
            "itself."
        ),
    )
    parser.add_argument(
        "--get-text",
        action="store_true",
        help=(
            "instead of a bounding box, print {'text': ...} -- the full textContent of the "
            "matching text node's parent element. Used for reading a live value out of a short, "
            "single-purpose element (e.g. \"Number of checkerboards collected: 7\") where the "
            "caller needs the actual number, not just whether/where the text appears -- "
            "wait_for_browser_text can only confirm presence of a fixed needle, not read a "
            "value that changes over time."
        ),
    )
    args = parser.parse_args()

    needle = json.dumps(args.contains)
    # Walks individual TEXT NODES (not elements) via TreeWalker, and measures
    # the needle's own Range within a matching node -- not
    # el.getBoundingClientRect() on whatever element happens to contain it.
    # An earlier version matched on element.textContent and took the
    # smallest matching element's whole box; that broke two ways in
    # practice: (1) if the enclosing element has no snug wrapper (e.g. a
    # wide clickable header bar with an icon + short heading text as its
    # only child), "smallest matching element" is still that whole wide
    # bar, not the text -- confirmed live against the BUI: a "Live view - "
    # lookup returned a 501x47 box (the entire panel header), centering the
    # point on a button several hundred px to the right of the actual
    # heading. (2) if the needle spans two elements (e.g. a terminal
    # command that visually wraps across two row divs), no single element's
    # own textContent contains it whole, so the smallest MATCHING ancestor
    # ends up being something huge (confirmed: a spanning terminal needle
    # returned a ~530x540 box, essentially the whole pane). Measuring a
    # Range within one text node fixes both: it's always exactly the
    # rendered glyphs of the substring, and a needle split across DOM
    # elements/text nodes simply finds no match at all (falls through to
    # point_at_browser_text's tuned-pixel fallback) instead of silently
    # returning a wildly wrong box.
    #
    # Overwrites `best` on every match rather than comparing -- so if the
    # needle appears in more than one text node (e.g. a short, repeated
    # camera name), the LAST one in document order wins. For a DOM-rendered
    # terminal, document order for row elements tracks visual top-to-bottom
    # order, so "last" reliably means "bottom-most / most recently
    # written," which is almost always the occurrence a caller wants.
    if args.get_href:
        # Same needle-matching walk as the bounding-box mode below, but
        # instead of measuring the text's own Range, walks up from the
        # matching text node to its nearest ancestor <a> and reads its
        # `.href` (the property, not getAttribute -- resolved to a full
        # absolute URL by the browser, so the caller never has to
        # reconstruct any app-specific path encoding itself). Same
        # last-match-wins tie-break as the bounding-box mode.
        expression = (
            "(function(){"
            f"var needle={needle};"
            "var walker=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT);"
            "var node,best=null;"
            "while(node=walker.nextNode()){"
            "var idx=node.nodeValue.lastIndexOf(needle);"
            "if(idx===-1)continue;"
            "var el=node.parentElement?node.parentElement.closest('a'):null;"
            "if(el){best={href:el.href};}"
            "}"
            "return best;"
            "})()"
        )
    elif args.get_text:
        # Same needle-matching walk as --get-href, but reads the matching text
        # node's parent's textContent instead of resolving a link -- right for
        # a counter rendered as its own dedicated element (e.g. a lone <div>),
        # where the parent's full text is exactly the value wanted, no more.
        expression = (
            "(function(){"
            f"var needle={needle};"
            "var walker=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT);"
            "var node,best=null;"
            "while(node=walker.nextNode()){"
            "var idx=node.nodeValue.lastIndexOf(needle);"
            "if(idx===-1)continue;"
            "if(node.parentElement){best={text:node.parentElement.textContent};}"
            "}"
            "return best;"
            "})()"
        )
    elif args.click:
        # Same last-match-wins needle walk as the other modes, but resolves
        # to the nearest ancestor matching --click-ancestor (default
        # <button>) instead of an <a> or a text Range. Dispatches a real
        # mousedown, a short real pause, then
        # mouseup + click -- not just a bare .click() -- so the browser's
        # native :active-pseudo-class press styling actually plays (a real
        # click's own mousedown/mouseup pair is what triggers it; .click()
        # alone fires only the synthetic "click" event and never touches
        # :active). An async IIFE so the pause is a real elapsed-time
        # setTimeout, not a busy-wait -- cdp_evaluate is called with
        # await_promise=True so CDP actually waits for it to resolve.
        ancestor = json.dumps(args.click_ancestor)
        expression = (
            "(async function(){"
            f"var needle={needle};"
            f"var ancestor={ancestor};"
            "var walker=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT);"
            "var node,best=null;"
            "while(node=walker.nextNode()){"
            "var idx=node.nodeValue.lastIndexOf(needle);"
            "if(idx===-1)continue;"
            "var el=node.parentElement?node.parentElement.closest(ancestor):null;"
            "if(el){best=el;}"
            "}"
            "if(!best)return null;"
            "window.confirm=function(){return true;};"
            "window.alert=function(){};"
            "var opts={bubbles:true,cancelable:true,view:window};"
            "best.dispatchEvent(new MouseEvent('mousedown',opts));"
            "await new Promise(function(r){setTimeout(r,150);});"
            "best.dispatchEvent(new MouseEvent('mouseup',opts));"
            "best.click();"
            "return true;"
            "})()"
        )
    else:
        # Scrolls the matched node into view (via its parent element) BEFORE
        # measuring the Range -- getClientRects() is viewport-relative, so a
        # needle that's currently scrolled out of view (e.g. a long
        # directory listing where the target sorts below the fold) would
        # otherwise measure to an off-screen/wrong position, and the caller
        # would move the mouse there instead of to the text's real, visible
        # location. `{block:'nearest',inline:'nearest'}` is a no-op if the
        # node is already fully visible (matches every existing tuned
        # pixel offset elsewhere in this pipeline, which assumed no
        # scrolling would happen for content already on-screen) and scrolls
        # the minimum amount otherwise -- works for both a plain scrolled
        # page and an element inside its own scrollable container (e.g. a
        # directory listing's own overflow:auto div), since scrollIntoView
        # walks every scrollable ancestor, not just the page itself.
        expression = (
            "(function(){"
            f"var needle={needle};"
            "var walker=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT);"
            "var node,bestNode=null,bestIdx=-1;"
            "while(node=walker.nextNode()){"
            "var idx=node.nodeValue.lastIndexOf(needle);"
            "if(idx===-1)continue;"
            "bestNode=node;bestIdx=idx;"
            "}"
            "if(!bestNode)return null;"
            "if(bestNode.parentElement){"
            "bestNode.parentElement.scrollIntoView({block:'nearest',inline:'nearest'});"
            "}"
            "var range=document.createRange();"
            "range.setStart(bestNode,bestIdx);"
            "range.setEnd(bestNode,bestIdx+needle.length);"
            "var rects=range.getClientRects();"
            "if(!rects.length)return null;"
            "var r=rects[rects.length-1];"
            "if(!(r.width>0&&r.height>0))return null;"
            "var best={x:r.x,y:r.y,width:r.width,height:r.height};"
            "best.chromeY=window.outerHeight-window.innerHeight;"
            "return best;"
            "})()"
        )

    try:
        value = cdp_evaluate(args.port, expression, await_promise=args.click)
    except Exception as e:
        print(f"ERROR: {e}", file=sys.stderr)
        sys.exit(1)

    if value is None:
        print(f"ERROR: no element found containing {args.contains!r}", file=sys.stderr)
        sys.exit(1)

    print(json.dumps(value))


if __name__ == "__main__":
    main()
