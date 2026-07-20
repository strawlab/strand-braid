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
  source (`braid_frontend/src/main.rs`) — `{encoded_name}` is
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

## Verified good in a real end-to-end run (2026-07-20)

- **`QR_SCROLL_CLICKS`/`QR_SCROLL_DELAY`** (400 clicks @ 0.03s): confirmed
  via frame extraction this reaches exactly the top of the log (the very
  first line, "Braid HTTP server listening at 0.0.0.0:1234", is visible
  at the top of the scrolled terminal) with no overshoot-and-wait — no
  retuning needed so far. Used symmetrically to scroll back down before
  Ctrl+C too; also confirmed correct via frame extraction (terminal
  showing live, current output at the bottom, not stuck mid-scroll).
- **`PER_CAMERA_DWELL_SECONDS`** (4): reads fine in practice — each
  camera's real live view (not a placeholder) is clearly visible for a
  beat before moving on. Not touched further.
- **`BROWSER_BACK_X/Y`** (40, 23) and **`BROWSER_CAMLINK_FALLBACK_X/Y`**:
  no longer load-bearing for the actual navigation (see above — that's
  programmatic now), only for where the mouse visually points during the
  caption. Looked reasonable in frame extractions; not worth more precise
  tuning unless a review says otherwise.

## Needs first-run tuning (still open)

- **`TERM_SYNC_FALLBACK_X/Y`, `TERM_QR_FALLBACK_X/Y`**: tuned pixel
  guesses used only if the corresponding CDP text lookup fails outright
  (see `point_at_browser_text`'s own fallback behavior in
  `lib/session.sh`). One of these WAS exercised in the 2026-07-20 run —
  the reopen-link lookup used the full token URL as its needle, which is
  long enough to wrap across two terminal rows (same failure mode
  `cdp_locate.py`'s Range-based matching documents for a spanning needle),
  logging `WARNING: CDP text lookup ... failed, using fallback
  coordinates`. Fixed by changing that needle to just `token=` (short,
  guaranteed single-row, shared by every line carrying this launch's
  token) — not yet re-verified with another real run, so re-check this
  specific step on the next one.

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
- Adds a close/reopen-the-GUI-window demonstration at the end, reusing
  `strand-cam-intro`'s exact pattern — the original stops right after the
  second launch's cameras synchronize, before ever showing this.
