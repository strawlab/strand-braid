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

## Update 2026-07-21 (later the same day): hold-first-frame + `StartPlayback` trigger

Both real runs above suffered from `ci2-video-file` starting playback the
instant strand-cam opened the camera — well before `record.sh` had
collapsed other panels, dismissed the frame-processing modal, or expanded
the Checkerboard Calibration panel, so the checkerboard-collection "timer"
effectively started at an arbitrary point relative to what was on screen,
not from when the recording was actually ready to watch. Fixed with a real
core-repo change (not just `record.sh`), reusing existing architecture
rather than inventing a new signal/file-based mechanism:

- **`camera/ci2-video-file`**: new `STRAND_CAM_VIDEO_FILE_AUTOSTART` env
  var (default `true`, fully backward compatible) — `"false"` holds
  `next_frame` on a clone of the very first decoded frame (unpaced,
  repeated indefinitely, `pending_frames`/`rx` untouched) until a
  `"StartPlayback"` `ci2::Camera::command_execute` call arrives, at which
  point pacing resets to start fresh from that moment rather than resuming
  from whenever the camera originally opened.
- **`camera/strand-cam-remote-control`**: new `CamArg::ExecuteCommand(String)`
  variant — a generic pass-through to `command_execute`, since no existing
  `CamArg` variant reached that trait method at all (it was previously only
  called internally, for PTP-sync commands).
- **`strand-cam/src/cam_arg_task.rs`**: one new dispatch arm forwarding
  `ExecuteCommand` to `cam.command_execute(&name, true)`, same
  log-and-continue error style as every other setter arm.
- **`lib/session.sh`**: new `post_cam_arg` helper — `curl`s
  `{"ToCamera": ...}` straight to strand-cam's `/callback` endpoint. This
  is confirmed to be the *exact same* route the BUI's own JS already uses
  for every camera command this pipeline otherwise simulates via
  `click_browser_element` (`strand-cam/src/strand-cam.rs:937`'s
  `callback_handler` → `cam_args_tx` → `cam_arg_task.rs` — the only control
  channel a running strand-cam process has), and needs no auth token since
  this pipeline always binds loopback-only
  (`AccessToken::NoToken` — `strand-cam.rs`'s `build_device_connect_urls`).
- **`record.sh`**: exports `STRAND_CAM_VIDEO_FILE_AUTOSTART=false`, and
  once every setting is configured (size fields shown), calls
  `post_cam_arg "$BUI_URL" '{"ExecuteCommand":"StartPlayback"}'` (captioned,
  since it's an HTTP call with no on-screen click to show) *before*
  enabling checkerboard calibration — deliberately reordered so detection
  only ever runs against the already-moving video, never the held first
  frame, regardless of whether that first frame happened to contain a
  detectable checkerboard pose of its own.

One real bug found and fixed during isolated verification (bypassing
`record.sh`, same isolation approach used throughout this backend's
development): the first version of the held branch applied **no pacing at
all** -- since it just returns a cached `Arc` clone immediately, the outer
processing pipeline called `next_frame` in a tight loop, confirmed via
`top` at **183% CPU** while held (should be idle). Fixed by applying the
exact same `Instant`-based pace-to-frame-rate logic to the held branch too
(repeating the same image on the same schedule a real frame would arrive
on), verified via the same isolated test: ~0% CPU while held, CPU usage
climbing normally (matching real ~8.57fps decoding) within seconds of
`curl`ing the `StartPlayback` trigger. Also verified: `cargo build --release
-p strand-cam --features checkercal`, `cargo clippy`, and `cargo fmt --check`
all clean.

Not yet re-run through `record.sh` end-to-end as of this note — next step
is the usual "run it, watch it" pass, which should also finally show
whether the still-unresolved "Saved camera calibration" gap above is a real
calibration-quality issue (now that detection genuinely only samples a
moving video) or something else.

## Update 2026-07-21 (later still): end-of-video detection switched from a
terminal log line to a marker file — the real cause of the "video never
reached its end" symptom

The re-run above (and the two before it) all shared one symptom: the
recording visibly showed the checkerboard video playing to completion and
correctly holding on its last frame, but `record.sh` itself always behaved
as if it hadn't — either exhausting the wait's timeout, or (once the
timeout was raised) still not detecting completion any sooner. Root-caused
without touching any code first: `wait_for_browser_text "$TERM_CDP_PORT"
"holding on last frame" ...` was polling `cdp_locate.py`'s DOM query
against the ttyd-bridged terminal, but `ttyd -t rendererType=dom` (via
xterm.js's DOM renderer) only ever materializes the *currently visible
viewport* as DOM nodes — scrolled-off history exists only in xterm.js's own
JS-side scrollback buffer, never as DOM text `cdp_locate.py`'s
`document.body` TreeWalker can see. Meanwhile
`strand-cam/src/frame_process_task.rs`'s checkerboard-detection loop logs
two `info!` lines roughly every 500ms ("Attempting to find NxM
chessboard." then a Found/Found-no-corners line) continuously — both while
the video plays *and*, since detection keeps running against the frozen
last frame, after it ends too. That's ~4 new terminal lines/second
relentlessly pushing the one-time "holding on last frame" line upward, out
of the visible viewport, and thus permanently out of `cdp_locate.py`'s
reach, within a few seconds of it ever appearing — no amount of extra
timeout budget fixes this, since the line is simply gone from the DOM, not
slow to arrive.

Fixed with the file-marker approach discussed with the user rather than
either of the two SSE/BUI-state "push" alternatives considered first (both
rejected on request, since they'd require touching the shared `ci2` trait
crate, `strand-cam.rs`'s single `next_frame()` call site, and/or the
`strand-cam-storetype` crate, and possibly the Yew frontend too — outside
this tutorial's own `ci2-video-file` backend, which the user does not want
touched for this):

- **`camera/ci2-video-file/src/lib.rs`**: new
  `STRAND_CAM_VIDEO_FILE_DONE_MARKER` env var (unset by default, fully
  backward compatible) — names a file path; `decode_loop` creates that file
  (empty contents, best-effort — a write failure just logs a
  `tracing::warn!` and otherwise doesn't affect playback) at the exact same
  moment it already logs "holding on last frame". A plain file's existence
  can't scroll out of view the way a terminal line can, so this sidesteps
  the DOM problem entirely without needing any code outside this one crate.
- **`lib/session.sh`**: new `wait_for_file FILE_PATH [TRIES] [INTERVAL]`
  helper (next to `wait_for_log_match`) — polls for plain file existence, no
  CDP/browser involved at all.
- **`record.sh`**: exports
  `STRAND_CAM_VIDEO_FILE_DONE_MARKER="$SESSION_WORK_DIR/video-file-ended"`
  alongside the existing `AUTOSTART`/`LOOP` exports, and the "watching
  checkerboard detections accumulate" wait now calls `wait_for_file
  "$CHECKERBOARD_DONE_MARKER" 200 1` instead of `wait_for_browser_text
  "$TERM_CDP_PORT" "holding on last frame" 200 1` (same 200-tries-*-1s
  bound).

Verified via a real end-to-end `record.sh` run (`cargo build --release -p
strand-cam --features checkercal` rebuilt explicitly first, to be certain
the running binary actually included this change rather than reusing a
stale one — worth doing every time, since `record.sh`'s own
`TARGET_DIR/strand-cam` existence check only detects a *missing* binary,
not a stale one): the "Watching checkerboard detections accumulate until
the video ends" step now resolves immediately once the video actually
finishes, with no timeout stall — `=== Video finished — holding on its
last frame ===` and `=== Number of checkerboards collected: 17 ===` both
printed right after it, and the whole run (raw capture) completed in
`213.77s` total. Clean teardown confirmed afterward (no leftover
strand-cam/ttyd/Xvfb/chrome processes owned by this session).

**Not fixed by this change, and still open:** `"Saved camera calibration to
file"` still never appeared in this run either (same `WARNING` as every
prior run) — that wait (`wait_for_browser_text "$TERM_CDP_PORT" "Saved
camera calibration" 15 2`) was deliberately left alone this round, but it
polls the exact same scrolling terminal DOM via the exact same mechanism,
so it's a strong candidate for the identical root cause. Worth applying the
same marker-file treatment there next, rather than assuming it's a genuine
calibration failure.

## Update 2026-07-21 (yet later): real calibration-save check, frame-
processing-error popup fixed at the source, and a file-navigator/viewer
step added after saving

The "Saved camera calibration" gap flagged just above turned out to be
exactly the same scrolling-DOM problem: fixed the same way as the
end-of-video wait, by checking the real file on disk instead of the
terminal log. `record.sh` now captures `CHECKERBOARD_CAL_YAML`'s mtime
*before* clicking "Perform and Save Calibration" (this file lives in a
real, persistent `~/.config/strand-cam/camera_info/` — not a fresh
per-run temp dir — so a stale file from an earlier run could already be
there; a bare existence check would false-positive on it) and
`lib/session.sh`'s new `wait_for_file_newer_than FILE BASELINE_MTIME`
waits for it to actually be rewritten. No more false-negative "may have
failed" warnings on a calibration that actually succeeded.

**Frame-processing-error popup**: root-caused and fixed at the source
instead of trying to catch/dismiss it visually. The old code waited up to
8s near BUI startup for the "frame processing too slow" modal, then
clicked "Ignore all future errors" + "Dismiss" — but that 8s window is
always well before checkerboard detection (the actual CPU-heavy trigger)
starts, so the modal was never actually there to click, and reappeared
later, undismissed, with nothing left to catch it. Fixed with a single
`post_cam_arg "$BUI_URL" '{"SetIngoreFutureFrameProcessingErrors":null}'`
call sent early (before anything CPU-heavy starts) — sets the backend's
`FrameProcessingErrorState` to `IgnoreAll` directly (same mechanism the
toggle-then-dismiss click sequence was trying to reach), so the modal
never appears on screen at all, which is also more honest: a real user
under real conditions basically never sees this, since it's purely an
artifact of this recording pipeline's own CPU overhead.

**Toggle sequencing fixed on request**: "Enable checkerboard calibration"
now fires *before* `StartPlayback` (not after) — detection has some
startup lag, and the held first frame never contains a checkerboard
anyway, so enabling it early means detection is already warmed up by the
time real playback starts, instead of missing the first several real
frames. A new "Save debug information" toggle is enabled alongside it
(one shared point/click/pause beat, not two separate ones), and both get
disabled the same way right after the video ends, before "Perform and
Save Calibration" (detection's job is done; calibration computes from
already-collected corners, not live detection).

**New: after saving, `record.sh` browses to and opens the calibration
file.** Considered and rejected two other approaches first, worth
remembering why:
- **AT-SPI automation of a real file manager (Nautilus)** — tried, hit
  real, escalating isolation problems: Nautilus is a GApplication
  singleton service (same class of bug as old `gnome-terminal`/Wayland
  issues) that leaked a window onto the user's *real* desktop on first
  attempt; the fix for that (a private `dbus-run-session`) then triggered
  GTK to spin up a second, redundant AT-SPI accessibility stack
  (bus-launcher + registry + dbus-daemon) as an unwanted side effect; and
  even a plain script on the *normal* shared session bus failed to
  connect to the accessibility bus for reasons never root-caused. Abandoned
  per the user's explicit call, after three consecutive rounds of
  side-effects on shared session state.
- **Launching a native program (e.g. a text editor) to view the file** —
  rejected because it has no CDP/DOM, so nothing inside it could be
  pointed at with the mouse the way everything else in this pipeline can.
- **What was actually built**: Chrome's own built-in `file://` directory
  listing (confirmed via live CDP query: real `<a>` elements, exact
  bounding boxes, same as every other click in this pipeline) serves as
  the file navigator — `lib/session.sh`'s new `open_file_navigator
  START_DIR` opens it in `--app` mode (hides the tab strip, same trick
  `open_terminal` uses for ttyd). `record.sh` clicks through
  `.config` → `strand-cam` → `camera_info` (real link clicks, confirmed
  hidden/dotfile folders do show up in Chrome's listing) to reach the
  calibration file. Chrome has no built-in viewer for `.yaml` though —
  confirmed it silently downloads the file instead of displaying it
  (`document.body` stays empty) — so selecting it doesn't let its own
  `href` navigate; instead `lib/render_file_viewer.py` (new, stdlib-only)
  reads the real file and builds a small HTML page (`<pre>` for
  text-like content, `<img>`/`<video>` for images/video, dispatched by
  extension — "robust to future usage not using yaml" per the user's
  request), which `open_file_viewer FILE_PATH` opens in a genuinely new
  isolated Chrome window (not a navigation within the navigator window,
  so it looks like a real "double-click opens a new window" experience).
  Because the result is real HTML, `point_at_browser_text` works on its
  content exactly like anywhere else — `record.sh` points at the
  calibration's own "Mean reprojection distance" line, its real quality
  metric.

Verified end-to-end via real captured frames (not just the script's own
exit code): the navigator correctly showed `.config/strand-cam/`, the
viewer window displayed the actual saved YAML content (cross-checked
against the real file on disk), and the mouse ended up on the
reprojection-distance line as intended.

**A real, still only partially-understood Chrome cosmetic bug found and
mitigated along the way**: once this pipeline started keeping 4 isolated
Chrome windows open at once (terminal, BUI, navigator, viewer) instead of
2, the BUI window started intermittently showing Chrome's "Restore pages?
Chrome didn't shut down correctly" infobar (`SessionCrashedBubbleView`) —
confirmed via real frame extraction, present from as early as t=20-30s of
a ~212s recording (i.e. not something caused by end-of-recording cleanup
ordering, which was the first hypothesis considered and ruled out on
timing grounds: the banner predates any cleanup by well over a minute).
Also confirmed genuinely intermittent, not deterministic: the *exact same*
code produced the banner on one run and not the very next one. The user's
working theory is a crash in some *unrelated* real Chrome process on this
machine tainting Crashpad (Chrome's crash-report handler, which runs as a
single process shared by every Chrome instance for a given Linux user,
not one scoped per isolated `--user-data-dir`) — plausible and consistent
with the intermittency, though not independently proven. Mitigated in
`_open_isolated_browser_window` (`lib/session.sh`) with four layered
measures, applied to every isolated Chrome window this pipeline opens:
`--disable-session-crashed-bubble` and `--disable-crash-reporter` (opts
out of the shared Crashpad infrastructure entirely), plus pre-seeding
both `<profile_dir>/Default/Preferences` (`exit_type: Normal`) and
`<profile_dir>/Local State` (`stability.exited_cleanly: true`) before
launch, since different Chrome versions/code paths can gate the restore
prompt on either file. Also added, unrelated but same code path:
`--disable-background-networking`/`--disable-component-update`/
`--no-default-browser-check`, fixing a long-known, previously-unaddressed
cosmetic gap from `strand-cam-intro`'s own original 2026-07-16 history (a
"Can't update Chrome" bubble). Multiple full runs after all of the above
came back completely clean (checked via frame extraction across the
whole video each time), but given the confirmed intermittency, this
should be treated as "significantly mitigated," not "provably eliminated,"
until it's been observed clean across many more runs.

## Update 2026-07-21 (still later): five real tuning runs -- offsets tuned,
click captions completed, a real debug-save confirmation wait, and capture
deferred past the panel-collapse step

Picked up on a fresh machine/session, working purely from user-given pixel
nudges plus one real UI-behavior question, verified via five consecutive
real end-to-end `record.sh` runs against `CHECKERBOARD_VIDEO=
Basler-81011970.mp4` (all clean: no CDP-lookup warnings, no leftover
processes, calibration saved each time -- checkerboard counts ranged
13-18 across runs, consistent with the same footage being decoded on a
loaded machine each time, not a regression).

**Every `point_at_browser_text` OFFSET_X/OFFSET_Y is now tuned** (this
scenario previously had every single point at the untouched library
default, `OFFSET_X=0, OFFSET_Y=6` -- see the old "Unverified" section
below, now out of date for offsets specifically). Current values, all in
`record.sh`:

| Point (needle) | Sweep | OFFSET_X | OFFSET_Y |
|---|---|---|---|
| "Checkerboard Calibration" (panel expand) | 0 | 0 | -2 |
| "Input: Checkerboard Size" (info only) | 50 (default) | 0 | 6 (default, untouched) |
| "Enable checkerboard calibration" (enable + disable, both occurrences) | 0 | 0 | -9 |
| "Save debug information" (enable) | 0 | 0 | -9 |
| "Save debug information" (disable) | 0 | 0 | -6 |
| "Number of checkerboards collected" | 100 | 15 | -10 |
| "Perform and Save Calibration" | 0 | 0 | -9 |
| Navigator: ".config" / "strand-cam" / "camera_info" (each) | 0 | 0 | -10 |
| Navigator: calibration filename | 0 | 0 | -10 |
| Viewer: needle changed from "Mean reprojection distance" to **"distance:"** | 50 (default) | 50 | 6 |

Note the asymmetry deliberately left in place: "Save debug information"'s
enable-click offset was never applied to its disable-click at the same
absolute value -- the user's tuning requests named specific call sites, not
"all occurrences of this needle," so don't assume the two should match
without asking. Same reasoning for why the two "Enable checkerboard
calibration" occurrences (enable/disable) DO share one value here -- that
one was requested for both explicitly. (A later session, see the
2026-07-23 update below, shifted every one of these five points up by a
further `-6`, applied uniformly to each occurrence's then-current value --
that's how the enable-toggle's `-3` and the disable-debug-toggle's `0`
above ended up at `-9`/`-6` respectively, preserving the original 3px gap
between them.)

**Click captioning completed.** Previously only the panel-expand,
Perform-and-Save, and navigator/viewer clicks had a "LEFT CLICK" caption;
the four toggle presses (enable/disable x "Enable checkerboard
calibration"/"Save debug information") had none. Added `log_event "LEFT
CLICK" 1.5` + `sleep 1.5` before each of those four clicks too, matching
the point-caption-pause-click pattern used everywhere else in this
pipeline. Conversely, removed the "Starting checkerboard video" caption
that used to precede the `StartPlayback` `post_cam_arg` call -- on
request, since that action has no on-screen click to pair the caption
with.

**New: a real wait for the "Save debug information" toggle's visible
"on" state**, not just its click. Investigated on request why the
toggle's orange styling and its "Saving debug data to {path}" text
appeared to trail the click in a captured video: both are driven by
`shared.checkerboard_save_debug` coming back from the backend
(`Toggle`'s `value`/`class` props in `web/ads-webasm/src/components/
toggle.rs` and `strand-cam/yew_frontend/src/main.rs:1258-1284`) via
`CamArg::ToggleCheckerboardDebug` -> `cam_arg_task.rs:671-698` (creates
the debug dir, updates shared state) -> a round-trip back to the browser
-- NOT by the native checkbox tick, which flips instantly on click
regardless of any of that. So there's a real, if usually brief,
backend round-trip between "checkbox ticks" and "orange + path text
appear." Added `wait_for_browser_text "$BROWSER_CDP_PORT" "Saving debug
data to" 20 1` right after the toggle clicks, before `StartPlayback`
fires -- not a hard gate (warns and proceeds on timeout, since this is a
cosmetic pacing improvement, not a correctness requirement).

**Screen capture (`start_capture`) moved twice this session, in
response to two separate requests, and now starts later than either
original position:**
1. First moved from immediately after `start_display` to immediately
   after `open_browser` (both windows open/tiled) -- on request, so the
   recording doesn't show launching strand-cam at all.
2. Then moved again, from right after `open_browser` to right after the
   "Collapsing other BUI panels" loop -- on request, so the recording
   also doesn't show the default expanded panel layout or the collapsing
   itself, only the tidied-up two-window layout, right before scrolling
   to "Checkerboard Calibration".

Both moves needed the same fix to avoid a real crash: `type_in` (typing
the launch command into the terminal) runs *before* `start_capture` now,
and it captions via `log_event`, which does `float($SESSION_CAPTURE_START_EPOCH)`
in Python -- crashes on the empty string that variable defaults to before
`start_capture` first runs. Fixed by setting
`SESSION_CAPTURE_START_EPOCH` to a placeholder value (`python3 -c 'import
time; print(time.time())'`) right after `start_display`, then truncating
`$SESSION_EVENTS_FILE` right after the real `start_capture` call so none
of the placeholder-timestamped pre-recording captions (the typed launch
command, "Enter") leak into the actual burned-in captions. Verified via a
real run: video duration dropped from the original ~211s down to ~205s
(capture starting after `open_browser`) and then ~202s (capture starting
after panel-collapse), consistent with progressively less dead time at
the front of the recording, not a regression.

All changes committed as `21288d92` ("tutorial-video-simulation: more
checkerboard-calibration tuning -- click captions, debug-save wait,
deferred capture start") on top of `c753d8de` (the offset-only tuning
commit from earlier the same session), both pushed to `origin/main` (the
fork).

## Update 2026-07-23: five points nudged up by 6, and the file navigator
## restyled to look like a real Linux file manager

Two changes this session, both from live video review, each verified via a
real end-to-end `record.sh` run against `CHECKERBOARD_VIDEO=
Basler-81011970.mp4` (clean: no CDP-lookup warnings, no leftover processes,
calibration saved each time).

**Offset tuning:** the "Perform and Save Calibration" button, both
occurrences of "Save debug information", and both occurrences of "Enable
checkerboard calibration" were all nudged up (`OFFSET_Y -= 6`) from
whatever their prior value was -- see the updated table above. Applied as a
uniform delta to each occurrence's own then-current value (not reset to a
shared number), so the pre-existing 3px gap between "Save debug
information"'s enable (`-3`) and disable (`0`) occurrences is preserved at
their new values (`-9`/`-6`).

**File navigator restyled as a fake GNOME Files ("Nautilus") window,
rather than Chrome's own bare `file://` directory listing.** Watching a
fresh run, the user flagged that the navigator step still read as "a
browser showing a raw file listing," not a native Linux file manager --
and specifically that no real file manager would let you casually open a
`.yaml` the way that looked like it was about to. Considered (again) and
re-rejected automating the real Nautilus app, for the same isolation
reasons as before (see the historical BLOCKED-adjacent section above:
AT-SPI's GApplication-singleton problems). Built instead: a new
`lib/render_nautilus_listing.py` (stdlib-only) that generates one HTML
page per directory level in the scenario's known navigation chain
(`$HOME` -> `.config` -> `strand-cam` -> `camera_info`), each page listing
that directory's **real** contents (`os.scandir`, sorted directories-first
then alphabetically, matching Nautilus's default icon-view sort) with real
folder/file icons pulled from this machine's actual installed `Yaru` icon
theme (`/usr/share/icons/Yaru`, falling back to `Adwaita`/`hicolor`, then a
minimal inline-SVG glyph if no theme is found at all -- confirmed this
machine has real `places/folder.png`/`mimetypes/application-x-yaml.png`
assets to use). Exactly one entry per page -- the scenario's known next
hop -- gets a genuine `<a href="file://...">` to the next generated page,
keeping `record.sh`'s existing `click_browser_element(needle,
ancestor_tag="a")` calls working completely unchanged; every other real
entry in the directory is still listed (name + icon) but inert, preserving
the existing "real clutter visible, real step-by-step navigation" property
rather than showing a fabricated listing. Breadcrumb ("Home ▸ .config ▸
strand-cam ▸ camera_info"), sidebar (the standard Nautilus bookmark list --
Recent/Starred/Home/Documents/Downloads/Music/Pictures/Videos/Trash), and
header back/forward buttons are all decorative/inert -- the recording
never navigates backward, so wiring real functionality there wasn't worth
the complexity.

One real efficiency bug found and fixed before this was verified against
this machine's actual home directory: the first version embedded a full
base64-encoded icon on every single grid item, and this machine's real
`$HOME` turns out to have **over 5000 entries** directly in it (years of
`.braid-*.log` files from real usage) -- that bloated the "Home" page to
7.3MB and would have made Chrome noticeably slow to render it. Fixed by
declaring each *distinct* icon exactly once as a CSS class
(`background-image: url(data:...)`) and referencing it by class per grid
item instead of repeating the data URI -- dropped the same page to 693KB,
generation time ~0.04s. Worth remembering if this script is ever extended:
never put a per-entry data URI directly in an `<img src>` for a listing
whose size isn't bounded.

Known, deliberate simplification: real Nautilus hides dotfiles by default
(needs Ctrl+H) -- this generator always shows them, same as the old raw
Chrome `file://` listing already did (the chain relies on `.config` being
visible with no "reveal hidden files" step). Not a regression, just not
literally stock Nautilus's default.

Scope was explicitly limited to the navigator window, confirmed with the
user -- the follow-on YAML viewer window (`open_file_viewer`/
`render_file_viewer.py`, plain `<pre>`-in-a-white-page, still shown in a
normal, non-`--app`-mode Chrome window) is unchanged and was explicitly
left out of this pass.

Verified via a real end-to-end run plus forward-seek `ffmpeg -ss` frame
extraction at all four navigation steps: real Yaru folder/YAML icons
rendered correctly (no broken-image icons), breadcrumb text correct at
each level, and the actual click targets still landed on the right real
directory entries throughout.

**Follow-on hardening, same session, prompted by the user asking directly
"will the target of our clicks always be in the visible portion of the
window?"** Answer required distinguishing two different mechanisms in
`lib/cdp_locate.py`: `click_browser_element` (used for the actual folder
navigation) dispatches a real DOM `mousedown`/`mouseup`/`.click()` directly
on the matched element handle -- this works regardless of scroll position,
since it's a programmatic DOM event, not a simulated click at a screen
pixel. `point_at_browser_text`'s underlying lookup (the plain,
non-`--click` mode), by contrast, measures the needle's `Range` via
`getClientRects()`, which is viewport-relative -- if the matched text were
ever scrolled out of view (e.g. a target sorting below the fold in a very
long generated listing), the mouse would visibly move to the wrong
on-screen position even though the click right after would still silently
succeed. For this scenario's current three folders this wasn't actually
happening (confirmed via the frame captures above -- all three sort near
the top of their respective real listings on this machine), but it wasn't
a guaranteed property of the design, so on request it was hardened at the
shared-library level rather than left as a known limitation: the lookup
now calls `bestNode.parentElement.scrollIntoView({block:'nearest',
inline:'nearest'})` on the matched node before measuring its `Range` --
a no-op if already fully visible (so it doesn't disturb any existing tuned
offset elsewhere in this pipeline that assumed no scrolling would occur),
and otherwise scrolls the minimum amount needed, walking every scrollable
ancestor (works for a plain page scroll AND an element nested in its own
`overflow:auto` container, e.g. this scenario's own `.content` div).

Verified with a standalone isolated reproduction (Xvfb + an isolated
Chrome window + a synthetic 200-row page with a target buried in a nested
`overflow-y:auto` div, ~8000px into its content, well past the container's
1083px viewport height) rather than trusting the fix by inspection alone:
confirmed the nested div's `scrollTop` was `0` before the lookup (target
would have measured at y≈8000 -- nowhere near the real, ~1083px-tall
viewport) and `6840` immediately after the lookup call, with the returned
bounding box (`y=1043, height=17`) landing right at the bottom edge of the
visible viewport as `{block:'nearest'}` should produce. Also reran this
scenario's own real `record.sh` afterward to confirm no regression --
clean, no leftover processes, same as every other run this session. This
change lives in the shared `lib/cdp_locate.py`, so it benefits every
scenario using `point_at_browser_text`, not just this one.

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

- **All `point_at_browser_text` calls still have no fallback pixel
  coordinates** (unlike every other scenario's `POINTING-NOTES.md`-tracked
  constants) — deliberately left unset rather than guessed. Note this is
  now stale in one respect: the `OFFSET_X`/`OFFSET_Y` values themselves
  *are* tuned (see the "five real tuning runs" update above, with the full
  current table) — it's specifically the `FALLBACK_X`/`FALLBACK_Y`
  parameters (used only if the CDP lookup itself fails) that remain unset.
  A failed CDP lookup will just warn and skip that one point (see
  `lib/session.sh`'s `point_at_browser_text`), not aim somewhere wrong —
  but it also means a firefox-fallback run (no CDP at all) would silently
  skip every pointing step in this scenario. Add real fallback coordinates
  once there's a captured frame to measure them from.
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
