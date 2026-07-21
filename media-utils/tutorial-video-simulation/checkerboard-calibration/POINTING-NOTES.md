# checkerboard-calibration: tuned constants and what still needs a real run

**Treat `lib/session.sh` and `checkerboard-calibration/record.sh` as the
source of truth for current behavior** if this file ever goes stale
relative to them.

**STATUS: the BLOCKED section right below is historical.** `record.sh` no
longer uses `v4l2loopback`/`ci2-webcam` at all -- it was rewritten to use
the `video-file` backend described in the "Update 2026-07-20 (later the
same day)" section further down, which resolved this blocker. The BLOCKED
writeup is kept for the diagnosis (in case `ci2-webcam`/`nokhwa` is ever
revisited for something else), not as a description of current behavior.

## BLOCKED (2026-07-20, historical): nokhwa can't open the v4l2loopback device

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

This is no longer something to pick up: `record.sh` was switched to the
`video-file` backend instead (see the update section right below), so
`ci2-webcam`/`nokhwa`/`v4l2loopback` are no longer part of this scenario at
all. `setup-v4l2loopback.sh` has been deleted. Kept here purely as a
diagnosis record in case `ci2-webcam`/`nokhwa` itself is ever revisited for
an unrelated reason.

### Update 2026-07-20 (later the same day): resolved at the `ci2` level —
### a new `ci2-video-file` backend bypasses `v4l2loopback`/`nokhwa` entirely

A new camera backend, `camera/ci2-video-file/` (`--camera-backend
video-file`), plays back a video file directly — no `v4l2loopback` kernel
module, no `ffmpeg` feeder process, no `nokhwa` involved at all. It decodes
via the existing `media-utils/frame-source` crate (previously wired up for
offline post-processing only, as this file's BLOCKED section above says —
this is the "completely different way to feed real footage into
strand-cam's acquisition pipeline" that section names as the alternative to
a `ci2-webcam`/`nokhwa` fix). It is purely additive: a new crate plus one
new `CameraBackend` enum variant and match arm in `strand-cam/src/cli_app.rs`
— no existing backend (including `ci2-webcam`) was touched, consistent
with the earlier decision not to modify existing strand-cam code for this.

Verified directly against strand-cam (bypassing `record.sh`/browser
automation, the same isolation approach used to originally diagnose the
`nokhwa` blocker above): `STRAND_CAM_VIDEO_FILE=.../intrinsic_cal_demo_trimmed.mp4
strand-cam --camera-backend video-file --camera-name intrinsic_cal_demo_trimmed`
enumerates and opens correctly (1920x1200), plays at the file's real ~8.57fps
(a loop-restart log line fired at ~131s wall-clock, matching the file's own
~120.6s duration — confirming correct pacing, not a wrong guess), and ran
with zero `cam_stream_task` "Channel full... Dropping frame data" errors
over multiple minutes and a full loop cycle.

Two real bugs were found and fixed while verifying this, worth knowing if
touching `ci2-video-file` again:
- `frame_source::FrameDataSource::average_framerate()` is only populated
  from strand-cam-specific SEI timing metadata (`h264_source.rs`'s
  `calc_avg_fps`) — `None` for an ordinary MP4 like this scenario's own
  `intrinsic_cal_demo_trimmed.mp4`. Using it naively (with a 30fps
  fallback) silently played this ~8.57fps-native video ~3.5x too fast.
  Fixed by preferring `average_framerate()` when present (e.g. for footage
  actually recorded by strand-cam itself) and otherwise estimating fps from
  two real consecutive frames' own timestamps.
- The `Instant`-based "absolute schedule" pacer (the same idiom `ci2-sim`
  uses) would blast through a backlog instead of resyncing if a downstream
  consumer ever stalled and delayed the caller — this is exactly what
  produced the "Channel full" flood during testing. Fixed with a resync
  guard: if the pacer is more than one frame period behind, it resyncs to
  "now" instead of dumping the whole backlog at once (matching how a real
  camera's small hardware buffer would just drop late frames rather than
  deliver them all in a burst).

**Done:** this scenario's own `record.sh` and `../README.md`'s "Checkerboard
calibration and the `video-file` backend" section were both updated to
actually use `STRAND_CAM_VIDEO_FILE`/`--camera-backend video-file` --
`record.sh` now exports `STRAND_CAM_VIDEO_FILE=$CHECKERBOARD_VIDEO`, detects
the exact `--camera-name` the backend expects via `--list-cameras` (the same
pattern `strand-cam-intro`/`braid-intro` use for a real camera's name,
rather than re-deriving the file-stem logic in bash), and its PATH-shadowing
wrapper now injects `--camera-backend video-file` instead of `webcam`. All
`v4l2loopback`/`ffmpeg`-feeder/loopback-device-detection code was removed,
along with `setup-v4l2loopback.sh` (deleted, no longer needed).

**Also new: a `BUILD_NEW_STRANDBRAID` toggle (default `true`), since this
scenario is a deliberate exception to the project's usual "prefer an
installed strand-cam" convention.** The `video-file` backend hasn't been
reviewed/merged upstream, so the real `.deb`-installed
`/usr/bin/strand-cam` on the primary dev machine predates it and rejects
`--camera-backend video-file` outright (confirmed directly) — and this
script must never rebuild/overwrite that installed binary. While
`BUILD_NEW_STRANDBRAID=true`, `record.sh` builds and uses its own local
copy from this repo instead (`target/release`, never on `PATH`); flip it to
`false` once the backend is approved and lands in whatever build ends up
installed. See `record.sh`'s own header comment and `../README.md`'s
"Checkerboard calibration and the `video-file` backend" section for the
full reasoning.

**Still the natural next step:** the usual "run it, watch it, fix
constants" tuning cycle the other two scenarios already went through —
this file's own "Unverified / needs a real run" section below still
applies unchanged.

## Update 2026-07-21: first two real end-to-end runs, several real bugs found and fixed

First real run (against a locally-built `strand-cam` with
`BUILD_NEW_STRANDBRAID=true`, real `trunk`-bundled BUI, `checkercal`
feature) completed end-to-end: checkerboard detections genuinely
accumulated 0→10 against `intrinsic_cal_demo_trimmed.mp4`. But the
recording ran 6m36s (way beyond a reasonable tutorial-video length) and the
user flagged two real problems from watching it:

- **The Checkerboard Calibration panel never visually opens.** Root cause:
  the whole panel is a CSS-checkbox-hack collapsible
  (`web/ads-webasm/scss/_wrap_collapsible.scss`'s `.wrap-collapsible` —
  `<CheckboxLabel label="Checkerboard Calibration">` renders a hidden
  `<input type=checkbox>` + sibling `<label>`, with every control inside in
  a `display:none` sibling `<div>` until checked). `scroll_until_visible`
  only confirms the heading text is present in the DOM — it never clicks to
  expand the section. This also explains why point_at_browser_text's CDP
  lookups for "Enable checkerboard calibration"/"Input: Checkerboard
  Size"/etc. kept failing in the first run (their bounding boxes are zero
  while `display:none`), even though the underlying `click_browser_element`
  calls still fired (a programmatic `.click()` on a hidden DOM node still
  dispatches its click handler in Chrome, unlike a real synthetic click).
  Fixed by adding an explicit click on the panel's own label (`ANCESTOR_TAG
  label`, same as the Toggle components inside it) right after scrolling to
  it, before touching anything inside.
- **A real "Error: frame processing too slow" modal** (`main.rs`'s
  `frame_processing_error_dialog`, a data-driven `.modal-container` fixed
  near the top of the viewport regardless of scroll) can appear under this
  pipeline's own CPU load (screen capture + browser + ffmpeg all competing).
  Added a bounded check (8 tries × 1s) right after the checkercal
  verification, before touching the Checkerboard Calibration panel at all:
  if present, toggles "Ignore all future errors" (a `<Toggle>`, `label`
  ancestor) *then* clicks "Dismiss" — checked the actual `Msg::
  DismissProcessingErrorModal`/`Msg::SetIgnoreAllFutureErrors` handlers in
  `main.rs` to confirm this ordering sends `SetIngoreFutureFrameProcessing
  Errors(None)` (permanent), not `Some(5)` (temporary), before relying on
  it. Needle "Dismiss" alone is ambiguous in principle (two other "Dismiss"
  buttons exist in `main.rs` — a JSON-decode-error modal and a
  version-update banner) but not in practice here, since `record.sh` already
  sets `DISABLE_VERSION_CHECK=1`.

The 6m36s runtime turned out to be dominated not by the checkerboard-
accumulation wait, but by the **"Confirming the save" step's default
5-minute `wait_for_browser_text` timeout** — the calibration save never
actually confirmed (see below), so it sat there the full 5 minutes before
giving up. Per the user's request ("set the timecap for the video to
2mins30sec, wait for 5 checkerboards"), tightened three things:
`CHECKERBOARD_MIN_COUNT` default `10` → `5`; `wait_for_checkerboard_count`
bounded to 60 tries × 1s (60s worst case, was 150×2s=300s); the save
confirmation's `wait_for_browser_text` bounded to 15 tries × 2s (30s worst
case, was 300s). Second run: **93s total** (raw video), comfortably under
the 2m30s target.

Also per user request, added a step (right after the checkercal
verification, before the frame-processing-modal check) that collapses every
other top-level BUI panel that defaults to expanded and actually renders in
this build — "MP4 Recording Options", "Post Triggering", "Object Detection",
"Camera Settings" (all `initially_checked=true` in `main.rs`; "AprilTag
Detection" never renders without the `apriltag` feature; "FMF & µFMF
Recording"/"ImOps Detection"/"Kalman tracking"/"Online LED triggering"
already default to collapsed) — via plain `click_browser_element` calls, no
visual pointing, since it's housekeeping for this recording's own clarity,
not something the tutorial is about. Not yet verified via a real run as of
this note (added same session as the sway-removal fix just below, before
the next `record.sh` invocation).

Also per user request, removed the left-right "sweep" (`point_at`'s
`SWEEP_WIDTH`, meant for "look at this text," not "about to click this" —
see `braid-intro`'s own established convention) from every
`point_at_browser_text` call that precedes a `click_browser_element`:
"Ignore all future errors", "Dismiss", "Checkerboard Calibration" (the
panel-expand click), "Enable checkerboard calibration", "Perform and Save
Calibration". Left the wiggle on the three calls that only ever indicate
text with no click following ("Input: Checkerboard Size", "Number of
checkerboards collected", "Saved camera calibration").

**Still not resolved:** "Saved camera calibration to file" has never
appeared in either real run, despite the Perform-and-Save click
demonstrably firing (script would have aborted under `set -e` otherwise,
since `click_browser_element`'s exit status is checked). Not yet
root-caused — could be a genuine calibration failure (e.g. the 5-10
collected poses being too similar/degenerate for the solver, since
`CHECKERBOARD_VIDEO`'s content/pacing was never verified against this
possibility) rather than an interaction bug. Next step once the above
fixes are verified: check the terminal log directly for an ERROR line from
the calibration attempt.

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
