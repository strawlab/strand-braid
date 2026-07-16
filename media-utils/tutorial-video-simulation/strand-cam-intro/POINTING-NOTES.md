# Mouse-pointing feature: status and next steps

Context: the tutorial video shows the mouse moving to and pointing at the
camera name on screen (browser heading, terminal log/typed text), rather
than sitting frozen or teleporting. This file tracks what's built, what's
solid, and what's still tuned-by-eye/fragile, to pick back up later.

## What's built (lib/session.sh)

- `move_mouse_to WINDOW_ID X Y` — exact pixel offset within a window
  (top-left origin), via `xdotool mousemove --window`.
- `move_mouse_into WINDOW_ID` — convenience wrapper for "just the center."
- `move_mouse_gradual TARGET_X TARGET_Y [STEPS] [STEP_DELAY]` — moves from
  the mouse's *current* position to an absolute screen position in small
  interpolated steps (default 20 steps × 0.05s), so motion reads as
  continuous dragging rather than teleporting — including across window
  boundaries, since it works in absolute screen coordinates.
- `point_at WINDOW_ID REL_X REL_Y [SWEEP_WIDTH]` — gradually moves to a
  point within WINDOW_ID, then sweeps left-right under it a couple of times
  (default sweep width 50px). REL_Y is expected to already be offset a
  little *below* the target text's baseline by the caller, so the sweep
  doesn't cover the text up.
- `point_at_browser_text WINDOW_ID NEEDLE [FALLBACK_X] [FALLBACK_Y]` — finds
  NEEDLE's on-screen bounding box via the Chrome DevTools Protocol
  (`lib/cdp_locate.py`) and calls `point_at` with the *real* position.
  Falls back to `point_at FALLBACK_X FALLBACK_Y` if CDP isn't available
  (firefox fallback browser doesn't speak this protocol) or the lookup
  fails for any reason (logged to `$SESSION_WORK_DIR/cdp_locate.log`,
  deleted on cleanup like everything else there).
- `scroll_page WINDOW_ID` — unrelated feature, already done; slowly
  scrolls a page down then back up (see git history / earlier project
  memory for that one).

`open_browser` (chrome/chromium path only) now also picks a free local TCP
port via a one-line `python3 -c 'import socket; ...'`, launches Chrome with
`--remote-debugging-port=$BROWSER_CDP_PORT`, and sets the global
`BROWSER_CDP_PORT` so `point_at_browser_text` can find it. Empty for the
firefox fallback.

## lib/cdp_locate.py

Hand-rolled minimal WebSocket client (stdlib only: `socket`, `hashlib`,
`base64` — no third-party deps, matching `burn_captions.py`'s convention),
since CDP's `Runtime.evaluate` needs a WebSocket connection and nothing
websocket-capable is installed on this machine (`websockets`/`websocket-client`
Python packages, `websocat`, `wscat`, `node` -- all checked, all missing on
2026-07-16).

Usage: `python3 cdp_locate.py --port PORT --contains "needle text"` — prints
`{"x":.., "y":.., "width":.., "height":.., "chromeY":..}` (CSS-pixel
viewport-relative bounding box of the *smallest* element whose textContent
contains the needle, to avoid grabbing a large wrapping container; plus
`window.outerHeight - window.innerHeight`, the browser's own chrome height,
needed to convert viewport-relative Y into window-relative Y).

Verified working end-to-end 2026-07-16: a standalone Xvfb+Chrome+test-HTML-page
test correctly picked the small `<h4>` over a larger wrapping `<div>` that
also contained the needle text, and the full `record.sh` pipeline run
produced a real, clean video with no leftover processes.

One real bug hit and fixed while building this: an extra stray `}` in the
hand-built JS expression string (miscounted brace nesting) caused a
`SyntaxError: Unexpected token 'if'` from `Runtime.evaluate` — worth
re-checking brace balance carefully if this JS expression is ever edited
further, since it's built by string concatenation, not written as a real
`.js` file, and syntax errors only surface at runtime via the CDP error
reply.

## record.sh wiring (current state)

```
BROWSER_CAMNAME_X=100        # tuned fallback only now (CDP is primary)
BROWSER_CAMNAME_Y=400
TERM_CAMNAME_X=340            # still tuned-by-eye, no CDP equivalent for xterm
TERM_CAMNAME_Y=300            # Command 1: points at log output ("got camera"/"run{cam=..}")
TERM_CAMNAME_Y2=500           # Command 2: points at just-typed text, before Return
```

Sequence:
1. Command 1 (`strand-cam`, no `--camera-name`) → `wait_for_url` →
   `open_browser` → point at browser heading (via CDP) → point at terminal
   log (tuned constant) → `scroll_page`.
2. Ctrl+C.
3. `type_only` (types Command 2's text, no Return yet) → point at terminal
   (tuned constant, `TERM_CAMNAME_Y2`) **before** activating → `xdotool key
   Return` → `wait_for_url` → `scroll_page`.

(User explicitly asked for exactly this ordering on Command 2: point at the
just-typed, not-yet-run command, not after it connects.)

## What's next: character-grid calibration for the terminal

The browser side is now robust (real DOM coordinates via CDP, adapts to any
layout change). The terminal side still uses tuned pixel constants, and the
two terminal points are NOT equally hard to make robust:

- **Command 2's point** (`TERM_CAMNAME_Y2`, pointing at text we ourselves
  just typed via `type_only`) — **fully deterministic and solvable**: we
  know the exact characters typed, and since it's always the newest content
  xterm auto-scrolls to show, it's reliably the last (or near-last) visible
  row. Plan: measure `xterm`'s actual character cell size once (launch a
  throwaway calibration window at a known `-geometry COLSxROWS`, read back
  its pixel `WIDTH`/`HEIGHT` via `xdotool getwindowgeometry`, divide) and
  compute the exact row from `SESSION_TERM_HEIGHT` / that char height,
  rather than guessing `TERM_CAMNAME_Y2=500`. This is the recommended next
  step — a real robustness win for a bounded amount of work.

- **Command 1's point** (`TERM_CAMNAME_Y`, pointing at strand-cam's own log
  output, e.g. `got camera simcam0` or a `run{cam="..."}` occurrence) — **no
  clean equivalent to CDP exists here.** xterm has no DOM-like introspection
  API; the row this text lands on depends on how many preceding log lines
  wrapped, which depends on strand-cam's exact log format (timestamps,
  message text) — something outside this tool's control and prone to
  silent drift if strand-cam's logging ever changes. Estimating it via
  known line lengths is *possible* but couples this tutorial tool to
  strand-cam's internal log format in a fragile way, arguably no more
  robust than the current tuned constant for the effort involved.
  **Recommendation discussed with the user 2026-07-16: leave this one on
  the tuned constant** rather than chasing precision that isn't really
  achievable without a strand-cam-side change (e.g. it exposing structured
  log positions somehow, which is out of scope). Revisit only if the tuned
  constant turns out to drift badly in practice.

## Other loose ends noted along the way

- `BROWSER_CAMNAME_X/Y` are now only a fallback (used if CDP fails or
  firefox is the active browser) — could eventually be removed as literal
  constants and replaced with a documented "last-resort guess" comment if
  CDP proves reliable across many runs, but no rush.
- The free-port-picking trick in `open_browser`
  (`python3 -c 'import socket; s.bind(("127.0.0.1",0)); ...'`) has a small
  window between closing that probe socket and Chrome binding the same
  port — acceptable for this single-user, low-concurrency use case, but
  worth knowing about if flakiness ever appears.
