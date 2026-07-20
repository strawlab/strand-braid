# checkerboard-calibration: tuned constants and what still needs a real run

**Treat `lib/session.sh` and `checkerboard-calibration/record.sh` as the
source of truth for current behavior** if this file ever goes stale
relative to them.

## BLOCKED (2026-07-20): nokhwa can't open the v4l2loopback device

First real run attempt happened this session, on Linux, with a real
`CHECKERBOARD_VIDEO` (a trimmed `intrinsic_cal_demo.mp4`) and a real
`v4l2loopback` device. Found and fixed two real bugs in `record.sh` itself
along the way (both already applied):

- **A stray apostrophe broke bash's parser.** Line 76's error message
  (`"...see this script's own header comment)"`) had an unescaped
  apostrophe inside a `${VAR:?message}` expansion. Bash misparsed quote
  boundaries across a large chunk of the file as a result, silently
  swallowing the `CHECKERBOARD_MIN_COUNT` assignment (line 87) into inert
  string content instead of executing it — surfacing later as a confusing
  `CHECKERBOARD_MIN_COUNT: unbound variable` error at the completely
  unrelated line where that variable is actually used. Fixed by rewording
  to avoid the apostrophe. **Lesson for any future edit to this file (or
  any `record.sh`): never put an apostrophe inside a `${VAR:?msg}` /
  `${VAR:-msg}` default/error string**, even inside outer double quotes —
  bash's parameter-expansion word parsing treats it as a real quote
  character, not a literal one.
- **`open_terminal` was called before the `strand-cam` PATH-shadowing
  wrapper was set up**, so the terminal's shell never picked up the
  `--camera-backend webcam` injection — it launched the *real* `strand-cam`
  with its default `pylon` backend instead, which then had no real Basler
  hardware to find. `strand-cam-intro/record.sh` gets this ordering right
  (wrapper/PATH export before `open_terminal`); this file didn't. Fixed by
  moving the wrapper setup before `open_terminal`.

With both fixed, `strand-cam --camera-backend webcam --camera-name
checkerboard-cam` (run directly, bypassing the recording harness entirely,
to isolate the problem) still fails immediately:

```
BackendError(Could not get device property CameraFormat: Failed to Fufill)
```

This comes from `nokhwa` (pinned at `0.10.11` via `ci2-webcam/Cargo.toml`)
during `Camera::new()` in `camera/ci2-webcam/src/lib.rs`'s `WrappedCamera::new`,
which requests `RequestedFormatType::AbsoluteHighestFrameRate` — negotiating
this requires nokhwa to enumerate the device's supported frame rates.

**Ruled out:** `v4l2loopback`'s `exclusive_caps` flag. Tested both
`exclusive_caps=1` (the documented default) and reloading the module
without it (`card_label` only, no `exclusive_caps`) — identical failure
either way.

**Confirmed the device itself works fine at the V4L2 level.** With
`v4l-utils` installed and `ffmpeg` actively feeding the loopback device
(same as `record.sh` does), `v4l2-ctl -d /dev/video9 --get-fmt-video`,
`--list-formats-ext`, and `--list-frameintervals` all succeed and report
sane values (`1920x1200`, `'YU12'`, a discrete `30.000 fps` interval).

**Working theory:** `v4l2-ctl` links against `libv4l2`, a compatibility
shim that smooths over driver quirks; `nokhwa`'s Linux backend
(`nokhwa-bindings-linux`) talks to the kernel via raw ioctls with no such
shim. `v4l2loopback` is a known-minimal/quirky V4L2 implementation, and
it's plausible nokhwa's raw-ioctl `AbsoluteHighestFrameRate` negotiation
path hits something `libv4l2` normally papers over. Not confirmed at the
ioctl level (would need e.g. `strace` on both `v4l2-ctl` and a small
nokhwa/`v4l` reproduction to compare the exact ioctl sequences and
responses).

**Not attempted:** changing `ci2-webcam/src/lib.rs` to request a specific/
closest format instead of `AbsoluteHighestFrameRate` (a different nokhwa
code path that might avoid whatever's failing). This would be a real
change to core `strand-cam`/`ci2-webcam` source, not just this recording
script, and the user explicitly decided **not** to modify existing
strand-cam code just to make this tutorial-video script work. So this
scenario is blocked pending either: a `nokhwa`/`ci2-webcam` fix landing
some other way, or a completely different way to feed real footage into
strand-cam's acquisition pipeline (see `../README.md`'s discussion — no
such alternative exists today; the closest thing, `media-utils/frame-source`,
is wired up for offline post-processing only, not live camera acquisition).

If picking this up again: the loopback device setup itself
(`setup-v4l2loopback.sh`) and the trimmed test video
(`intrinsic_cal_demo_trimmed.mp4`, already in this directory) are both
ready to go — the blocker is purely the `nokhwa` open call above. Re-verify
first with the direct `strand-cam --camera-backend webcam --camera-name
checkerboard-cam` command (no `record.sh`, no browser automation) before
assuming anything's changed.

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
