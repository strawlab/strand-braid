# braid-intro: tuned constants and what still needs a real run

This scenario introduces more tuned-by-eye constants than
`strand-cam-intro` had at launch, and each iteration is slower (real PTP
hardware has to actually resynchronize on every run). This file tracks
what's solid (verified via source/DOM structure, not just guessed) vs.
what's a first guess that needs watching a real generated video to tune —
read this before changing any constant at the top of `record.sh`.

**Treat `lib/session.sh` and `braid-intro/record.sh` as the source of
truth for current behavior** if this file ever goes stale relative to them
— update this file alongside any constant change, the same way
`strand-cam-intro/POINTING-NOTES.md` was kept in sync historically.

## Solid (CDP-based, or otherwise verified, not tuned-by-eye)

- The camera list order in the GUI matches `config.TOML`'s `[[cameras]]`
  order exactly (confirmed against a real screenshot of the original
  Video_2.mp4) — `record.sh` parses `CAMERA_NAMES` from the config at
  runtime rather than hardcoding it, so this holds regardless of which
  config is pointed at via `BRAID_CONFIG_TOML`.
- The two `Predicted URL` extractions (one per launch) are exact —
  `wait_for_log_match` polls `braid-run`'s own `~/.braid-*.log`, not a
  guess, and `newest_file_matching`'s `NEWER_THAN_EPOCH` gate means launch
  2's lookup can't accidentally re-find launch 1's file.
- **Camera navigation is programmatic, not a real click** — confirmed live
  (first real end-to-end run) that a literal `xdotool click 1` on the
  camera name in this app's actual Yew/WASM-rendered GUI did not trigger
  real navigation (the dashboard stayed on the camera-list page the whole
  time, frame counters still climbing, URL bar never changing). The
  camera name IS a genuine `<a href="/cam-proxy/{encoded_name}/">` per
  source (`braid/braid-run/braid_frontend/src/main.rs`) — `{encoded_name}` is
  percent-encoded (confirmed live: `Basler-40290626` renders as
  `Basler%2D40290626` in the resolved href), so reconstructing it by hand
  in bash wasn't worth it either. Fixed by reading the anchor's real
  resolved `.href` via a new `get_browser_href` helper
  (`lib/cdp_locate.py --get-href`) and navigating there with a new
  `navigate_browser` helper (`window.location.href = ...` over CDP) —
  verified this produces a real page load (title becomes "Basler-XXXXXXXX
  - Strand Cam", a real live camera image renders) and a real history
  entry. `browser_back` was changed to match: it now calls
  `navigate_browser` back to the launch's own list URL instead of
  `xdotool key alt+Left`, so both directions use the same verified
  mechanism rather than mixing a real keystroke with a programmatic
  step. Verified end-to-end across all 5 cameras in a full real-hardware
  run.
- **Quit Braid is also a programmatic click, for the same reason** — a
  real xdotool click risks the same WASM-event-routing unreliability as
  the camera links. `cdp_locate.py --click` / `click_browser_element()`
  find the button by the same needle-matching as text-pointing and click
  it via CDP, dispatching a real `mousedown` → 150ms pause → `mouseup` →
  `click` (not a bare `.click()`) so the browser's native `:active` press
  styling actually plays. Also auto-overrides `window.confirm`/`alert`
  first, since the real click handler pops a native, JS-blocking
  `confirm()` dialog ("Quit Braid and all connected cameras?") that this
  hand-rolled CDP client has no `Page.javascriptDialogOpening` handling to
  intercept otherwise. Verified end-to-end: the "Braid has quit" text
  appears afterward (`wait_for_browser_text` checks for it before capture
  stops).
- **The camera-link/QR-link/reopen-link CDP needles must not be a bare
  `"http://"`** — strand-cam's own `Will connect to braid at
  "http://127.0.0.1:PORT/..."` log line (printed once per camera,
  repeatedly) contains `http://` too and is far more recent/bottom-anchored
  than the desired `"QR code for {url}"` line, so a bare needle silently
  matched the wrong (loopback) line. Fixed by using `"r http://"` — the
  only one of the three `http://`-containing log lines
  (`Predicted URL: http://`, `Will connect to braid at "http://`, `QR code
  for http://`) where an `r` immediately precedes ` http://`.
- **Browser back-button position** (`BROWSER_BACK_X/Y` = 27, 68): measured
  directly from a captured frame (`raw.mp4` at t=52s, during camera 1's
  dwell, before the back-button click itself moves the cursor there — see
  `record.sh`'s own header comment for the general offset convention).
  Real arrow center: absolute `(1023, 140)`; browser window origin:
  `(996, 72)` (`SESSION_MARGIN*2 + SESSION_PANE_WIDTH`, `SESSION_MARGIN` —
  zero frame-extent decoration on this normal, non-app-mode window, so
  also its content origin). This superseded an earlier guessed `(40, 23)`,
  which was landing in the tab strip, not the toolbar row the real arrow
  sits in (not the same row as `BROWSER_CLOSE_Y`, despite both being "top
  of the browser chrome").

## Verified good in a real end-to-end run (2026-07-20, after tuning)

- **`scroll_until_visible()`** (replaces the original plan of a fixed
  `scroll_by` count for both QR-reveal scrolls): scrolls in small batches,
  checking via `cdp_locate.py` after each one, stopping as soon as the
  `"r http://"` needle is rendered. The original fixed-count approach
  (`QR_SCROLL_CLICKS=400 @ 0.03s`) reached the top almost immediately this
  early in the log, then sat visibly frozen for the rest of the nominal
  12s scroll (confirmed via 1-fps frame extraction: static for ~15-19s
  before the next action even began) — purely wasted time once the
  terminal's already hit its scroll limit and every further wheel event is
  a no-op. Stopping at the first match also fixes a correctness bug on the
  re-scroll: scrolling all the way to the absolute top landed on launch
  1's older QR/URL block instead of launch 2's own nearer one.
  `QR_SCROLL_CLICKS` is kept as `scroll_until_visible`'s `MAX_CLICKS` safety
  ceiling (same "harmless to overshoot" reasoning as before), just no
  longer the typical actual cost. The scroll-back-down-before-Ctrl+C still
  uses plain `scroll_by` (no specific text to stop at, just "go to the
  current bottom").
- **`PER_CAMERA_DWELL_SECONDS`** (4): reads fine in practice — each
  camera's real live view (not a placeholder) is clearly visible for a
  beat before moving on. Not touched further.
- **`BROWSER_CAMLIST_SCROLL_CLICKS`** (3, scrolls down before every camera
  link): has to run on *every* loop iteration, not once before the loop —
  `browser_back`'s full-page reload (`navigate_browser` setting
  `window.location.href`) resets scroll to the top each time, so later
  cameras would fall back out of view otherwise.
- **Point offsets**, current values after several rounds of "watch video,
  nudge, rerun": sync-line point `OFFSET_Y=1` (default is 6; halved twice
  from there); http-link points (QR reveal + reopen-link) `OFFSET_Y=-6`;
  camera-name points `OFFSET_Y=-12`; Quit Braid button `OFFSET_Y=-4`.
  Note `point_at_browser_text` (`lib/session.sh`) has a baked-in `+6`
  baseline buffer added below the measured text *regardless* of the
  caller's `OFFSET_Y` (to clear glyph descenders) — `OFFSET_Y=0` still
  lands ~6px low. These are real screen-pixel amounts, not scaled to any
  particular text size, so if the GUI's font size or these particular
  strings ever change noticeably, expect to retune.
- **`BROWSER_QUIT_SCROLL_CLICKS`** (30, plain `scroll_by`, not
  `scroll_until_visible`): reaches the bottom of the dashboard page
  (past Recording/Cameras/Status) to reveal the Quit button. Not yet
  switched to `scroll_until_visible` — could be, using needle `"Quit
  Braid"` — but wasn't flagged as causing a visible freeze the way the
  terminal QR scrolls were, so left as-is.

## Update 2026-07-22: sim-camera fallback, capture-timing fix, and a
## post-Quit-Braid terminal scroll (not yet in the sections above)

Three changes this session, committed as `f67a7264`/`aac9c214`/`7d62415b`,
not folded into the "Solid"/"Verified good" sections above since they
weren't re-tuned as part of that same tuning pass:

- **Hardware-free sim fallback added** (`f67a7264`): `record.sh` now
  auto-detects real Basler hardware + the real config file (same
  `--list-cameras` check `strand-cam-intro` uses) and falls back to a
  generated sim config otherwise (`braid-sim generate` -- 5 `ci2-sim`
  cameras, `FakeSync` triggering, no PTP hardware needed).
  `BRAID_CAMERAS=sim` forces the fallback explicitly; an explicit
  `BRAID_CONFIG_TOML` still always wins outright. Two real bugs found and
  fixed via actual end-to-end runs: the generated sim config bound
  `http_api_server_addr` to `127.0.0.1` only, so `braid-run`'s "QR code for
  {url}" needle (only printed for a non-loopback URL) was never found and
  the script hung -- fixed by binding `0.0.0.0` instead. Separately,
  `navigate_browser`'s `window.location.href` eval can lose its own CDP
  reply when Chrome tears down the page's execution context to start
  navigating, timing out under `set -e` even though the navigation itself
  already happened -- reproduced deterministically on the 5th camera
  across two runs; fixed with a bounded retry in `lib/session.sh` (shared
  code, so this also hardens the real-hardware path).
- **Screen capture now starts after the terminal window settles**
  (`aac9c214`, shared with `strand-cam-intro`): both scenarios used to
  start capture before `open_terminal` ran, showing the ttyd-bridged
  Chrome window jump from its default size/position into its final tiled
  layout on camera. Now `open_terminal` runs first, capture starts after,
  plus a 0.5s hold on the placed, empty terminal before typing begins.
  Also fixed a related artifact: `open_terminal`'s own
  `windowmove`/`windowsize` calls trigger openbox's transient
  "WIDTHxHEIGHT" resize-indicator overlay, which was still fading in the
  opening ~0.3-0.5s of the recording -- added a settle sleep at the end of
  `open_terminal` itself (in `lib/session.sh`, so every scenario benefits).
- **Terminal scrolled to bottom after "Quit Braid"** (`7d62415b`): after
  clicking Quit Braid and confirming the quit text, the mouse moves to the
  terminal and scrolls it to the bottom (reusing
  `QR_SCROLL_CLICKS`/`DELAY`, the same fixed-count `scroll_by` already used
  to reliably reach the real bottom before Ctrl+C), showing `braid-run`'s
  own shutdown log output before capture stops.

All three verified via real end-to-end runs against the installed
`braid-run`/`strand-cam` and the real 5-camera rig (the sim-fallback
addition also against the hardware-free path) -- clean, no leftover
processes.

## Known deviations from the original Video_2.mp4 (intentional)

- Cycles through **all 5 cameras**; the original skips the first one in
  the list (`Basler-40290624`) — confirmed via frame-by-frame review this
  wasn't a deliberate choice in the original, so the new video is more
  complete instead of replicating the omission.
- Window layout is the same tiled terminal-left/browser-right convention
  `strand-cam-intro` uses, not the original's full-width browser window
  overlapping most of the terminal — the user asked for strand-cam-intro's
  window-handling approach specifically, not a pixel recreation of the old
  recording's layout.
- Adds a close/reopen-the-GUI-window demonstration, and then a full
  scroll-to-bottom-and-click-"Quit Braid" ending, reusing
  `strand-cam-intro`'s close/reopen pattern for the former — the original
  stops right after the second launch's cameras synchronize, before ever
  showing either.

## Not yet done

- No full frame-by-frame comparison against the real original `Video_2.mp4`
  for pacing/captions, unlike `strand-cam-intro` (which got one — see its
  own `COMPARISON-NOTES.md`). Tuning so far has been iterative
  round-by-round feedback against generated output, not a systematic pass.
- `BROWSER_QUIT_SCROLL_CLICKS`/`BROWSER_QUIT_FALLBACK_X/Y` haven't been
  put through as many rounds of scrutiny as the terminal-side constants
  above — worth a closer look if the Quit-button ending ever looks off.
