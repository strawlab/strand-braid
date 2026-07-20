# checkerboard-calibration: tuned constants and what still needs a real run

**Nothing in this scenario has been run yet.** It was written on macOS,
where this whole pipeline cannot run at all (see `../README.md`'s
Prerequisites — no `Xvfb`/`x11grab`), and no `CHECKERBOARD_VIDEO` was
available yet either. Treat all of `record.sh` as an untested first draft
until someone actually runs it on Linux against real footage and reports
back — this file exists so that first real run's findings have somewhere
to go, the same way `strand-cam-intro/POINTING-NOTES.md` and
`braid-intro/POINTING-NOTES.md` accumulated theirs.

**Treat `lib/session.sh` and `checkerboard-calibration/record.sh` as the
source of truth for current behavior** if this file ever goes stale
relative to them.

## Solid (verified via source, not tuned-by-eye)

- The BUI markup for the "Checkerboard Calibration" panel
  (`strand-cam/yew_frontend/src/main.rs:1264-1320`), its default 8x6 board
  size (`strand-cam/strand-cam-storetype/src/lib.rs`'s `CheckerboardCalState`
  default), the `checkercal` cargo feature gating it
  (`strand-cam/Cargo.toml:147`, not in `default = [...]`), and the terminal's
  own confirmation log line (`info!("Saved camera calibration to file: {}",
  ...)` in `strand-cam/src/cam_arg_task.rs`) are all read directly from
  source, not guessed.
- **"Enable checkerboard calibration" needs `click_browser_element`'s new
  `ANCESTOR_TAG=label` argument, not the default `button`** — this control
  is a `<Toggle>` (`web/ads-webasm/src/components/toggle.rs`), which renders
  `<label><input type=checkbox></label>` with no `<button>` anywhere in its
  DOM. `cdp_locate.py --click` and `session.sh`'s `click_browser_element`
  were both extended (this session) to accept a configurable
  `--click-ancestor`/`ANCESTOR_TAG` for exactly this case — clicking the
  `<label>` natively activates its `<input>` per HTML's own label-click
  behavior, so no need to locate the `<input>` itself.
- **Reading the live "checkerboards collected" count needs a new
  `get_browser_text`/`--get-text` mode** — `wait_for_browser_text` can only
  confirm a fixed needle is present, not read a value that changes over
  time (the count itself). Added `cdp_locate.py --get-text` (prints the
  matching text node's parent's full `textContent`) and a
  `get_browser_text` wrapper in `session.sh`; `record.sh`'s own
  `wait_for_checkerboard_count()` polls that and regex-extracts the number.
  Untested against the real page structure — if the surrounding
  `<div>{num_checkerboards_collected}</div>` (main.rs:1303-1305) doesn't
  round-trip cleanly through `--get-text`, this is the first thing to check.

## Unverified / needs a real run

- **All `point_at_browser_text` calls have no fallback pixel coordinates**
  (unlike every other scenario's `POINTING-NOTES.md`-tracked constants) —
  deliberately left unset rather than guessed, since there was no way to
  visually review a real frame while writing this. A failed CDP lookup will
  just warn and skip that one point (see `lib/session.sh`'s
  `point_at_browser_text`), not aim somewhere wrong — but it also means a
  firefox-fallback run (no CDP at all) would silently skip every pointing
  step in this scenario. Add real fallback coordinates once there's a
  captured frame to measure them from.
- **Whether `scroll_until_visible ... 60` is enough** to reach the
  "Checkerboard Calibration" heading — chosen by analogy to
  `braid-intro`'s `BROWSER_QUIT_SCROLL_CLICKS=30` (a similarly-deep BUI-page
  scroll, not a terminal-log scroll where `braid-intro`'s `400` applies),
  doubled for margin since this panel may sit further down the page than
  braid-run's Quit button. A stated judgment call, not a measurement —
  revisit if the first run's log shows `scroll_until_visible` exhausting
  its max without finding the heading.
- **Whether `v4l2loopback`'s `exclusive_caps=1` is actually required** for
  nokhwa (`ci2-webcam`) to enumerate the loopback device as a usable camera
  — included in `record.sh`'s own suggested `modprobe` command based on
  general v4l2loopback/video-conferencing-tool lore, not confirmed against
  this specific stack. If `strand-cam --camera-backend webcam --list-cameras`
  doesn't show the loopback device at all, try without it (or check `dmesg`
  for what nokhwa's V4L2 backend actually rejected it for).
- **Detection timing**: `checkerboard_loop_dur` in
  `strand-cam/src/frame_process_task.rs` samples at most once every 500ms —
  whatever `CHECKERBOARD_VIDEO` ends up being needs genuinely distinct,
  reasonably-held (>=1s) checkerboard poses, not continuous fast motion, or
  the "checkerboards collected" counter may barely move. Worth checking the
  counter's growth rate in the first real run's own stderr
  (`wait_for_checkerboard_count`'s progress line) before assuming the video
  itself is fine.
- **`CHECKERBOARD_MIN_COUNT=10`** (matches the docs' "say, at least 10") is
  a stated default, not something confirmed to produce a good calibration
  with whatever footage ends up being used — the real
  `docs/user-docs/users-guide/src/braid_calibration.md:64-65` phrasing is
  itself just a rule of thumb, not a hard requirement.

## Known gap vs. what a "regenerated" tutorial video would normally mean

Unlike `strand-cam-intro`/`braid-intro`, there is **no pre-existing
"Video_3.mp4" in this repo** for this scenario to regenerate — the user has
an old reference video (`Video_3.mp4`, off-repo, on a lab file server) "of
the kind of thing" this should show, plus an example calibration debug
folder from a real session (`Basler-40454395.yaml`,
`checkerboard_debug_*/input_8_6_*.png`) alongside it, but that reference
hasn't been frame-reviewed against this script's own pacing/captions the
way `strand-cam-intro/COMPARISON-NOTES.md` did for `Video_1.mp4`. Worth a
comparison pass once both a real `CHECKERBOARD_VIDEO` and a first real
`record.sh` output exist.
