# Onboarding: tutorial-video-simulation

Read this first if you're picking up this work cold (including on a
different machine) — it's the portable context that doesn't live in any
one machine's local memory/notes.

## What this is

Regenerates strand-braid's stale CLI/GUI tutorial videos by driving the
real `strand-cam`/`braid-run` binaries and a real browser end-to-end
(`xdotool` + `ffmpeg` x11grab + Chrome DevTools Protocol), instead of
hand re-recording. See `README.md` in this directory for the full
prerequisites/architecture writeup (ttyd-bridged terminal, CDP-based
pointing, isolated Xvfb display, etc.) — this file is a status/handoff
summary, not a replacement for it.

**Lives only on the fork `Mharrap/strand-braid`, not upstream.** Don't
push to `strawlab/strand-braid` or open a PR there without the user's
explicit go-ahead — only the fork is safe to land commits on for now.
Always push to `origin` (should already point at
`git@github.com:Mharrap/strand-braid.git`).

## Three scenarios, very different hardware requirements

- **`strand-cam-intro/record.sh`** — well-verified, stable since
  2026-07-17. Auto-detects real Basler hardware via `--list-cameras`,
  falling back to the hardware-free `sim` backend if none is found. **Runs
  fine on a machine with no camera hardware at all.**
- **`braid-intro/record.sh`** — added and heavily tuned 2026-07-20. **No
  hardware-free fallback.** Requires:
  - 5 real Basler cameras physically attached (this session's dev machine
    has serials `40290624/26/76/39/95`, hardcoded nowhere but relied on via
    a config file — see next point).
  - A Braid config TOML at `/home/strawlab/BRAID_TOMLS/config.TOML` (or
    override via `BRAID_CONFIG_TOML=...`) describing those 5 cameras with
    `PtpSync` triggering.
  - A real extrinsic calibration file at
    `/home/strawlab/BRAID_EXT_CALIBRATIONS/...` (path is inside the config
    TOML).
  - **If you're on a different machine without this exact hardware/config,
    `braid-intro/record.sh` cannot run** — camera sync will simply hang
    until timeout. Don't assume a script regression if it fails there;
    check hardware/config first.
- **`checkerboard-calibration/record.sh`** — added 2026-07-20, no camera
  hardware requirement of its own at all: it plays a real recorded
  checkerboard video directly through strand-cam's `video-file` backend
  (`camera/ci2-video-file`, `--camera-backend video-file`), which decodes
  the file itself via `media-utils/frame-source` and paces playback to its
  own native frame rate — no virtual camera device, `ffmpeg` feeder
  process, or `nokhwa` involved at all. (An earlier version fed the video
  through a `v4l2loopback` virtual camera device into strand-cam's `webcam`
  backend instead; `nokhwa` failed to open that device at all — see
  `POINTING-NOTES.md`'s "BLOCKED" section, now historical, for the full
  diagnosis. The `video-file` backend was added specifically to unblock
  this, then `record.sh` itself was updated to use it — see "Current
  state" below.) **Run end-to-end four times now** (2026-07-21), each time
  fixing a real bug the user caught by watching the output or diagnosed
  from the symptom: the Checkerboard Calibration panel's collapsible
  section wasn't actually being expanded, a "frame processing too slow"
  modal was left undismissed, the recording ran way too long (6m36s)
  because of an open-ended wait that's since been bounded, a
  hold-on-first-frame + `"StartPlayback"` trigger was added so the video
  only starts moving once `record.sh` has finished configuring everything
  else, and — most recently — end-of-video detection was switched from
  polling the ttyd terminal's rendered DOM for a log line to polling for a
  plain marker file `ci2-video-file` now writes directly, since the
  terminal-DOM approach was silently unreliable (xterm.js's DOM renderer
  only ever shows the current viewport, and the checkerboard-detection
  loop's own ~4 lines/second of logging scrolled the one-time "holding on
  last frame" line out of reach within seconds of it appearing, no matter
  how long the timeout was) — see `POINTING-NOTES.md`'s dated update
  sections for the full history of all of these. "Saved camera calibration
  to file" has still never appeared in any run so far; still unresolved,
  and a strong candidate for the identical scrolling-DOM root cause (see
  `POINTING-NOTES.md`'s latest update).

  **Unlike every other scenario here, this one does NOT default to
  preferring an installed strand-cam.** The `video-file` backend is new and
  not yet reviewed/merged upstream, so the real installed `.deb` build on
  the primary dev machine (`/usr/bin/strand-cam`) predates it and rejects
  `--camera-backend video-file` outright — confirmed directly. This script
  must never rebuild/overwrite that installed binary. A new
  `BUILD_NEW_STRANDBRAID` toggle (default `true`) makes `record.sh` build
  and use its own local copy from this repo instead (`target/release`,
  never on `PATH`); set it to `false` once the backend is approved and
  lands in whatever build ends up installed, to switch back to the normal
  prefer-the-installed-build behavior. See `record.sh`'s own header comment
  for the full reasoning.

  Also the only one of the three that isn't regenerating a pre-existing
  tutorial video — there's no earlier "Video_3.mp4" in this repo.

## Before running either script

Check for a real `braid-run`/`strand-cam` process already using the
cameras or ports 1234 (Braid HTTP)/3440 (strand-cam) — this machine
sometimes has one running from manual testing, and starting `record.sh`
alongside it causes a silent port conflict or camera-open failure:

```
ss -ltnp | grep -E ':3440|:1234'
ps aux | grep -E 'strand-cam|braid-run'
```

If something's running that you didn't start, **ask before touching it** —
don't kill it yourself (see "no broad process kills" below).

## Current state (as of 2026-07-20, `checkerboard-calibration` added this session)

`strand-cam-intro` and `braid-intro` both run end-to-end cleanly against
real hardware, zero leftover processes. `braid-intro` went through many
rounds of "run it, watch the generated video, get specific feedback, fix,
rerun" this session. Notable fixes worth knowing about if you're touching
this code again:

- **A real correctness bug, not just cosmetic:** any CDP text-lookup that
  used a bare `"http://"` needle would match strand-cam's own far more
  frequent `Will connect to braid at "http://127.0.0.1:PORT/..."` log line
  instead of the intended `"QR code for {url}"` line. Fixed by using needle
  `"r http://"` (only `"QR code for"` has an `r` immediately before
  ` http://`). If you add any new CDP text lookup involving a URL, check
  what else on screen might contain `http://` before picking a needle.
- **`scroll_until_visible()`** (new helper, `lib/session.sh`): scrolls in
  small batches, checking via `cdp_locate.py` after each one, instead of
  a fixed click count. Prefer this over `scroll_by` whenever you're
  scrolling *toward* a specific piece of text — a fixed click count either
  leaves the recording visibly frozen once it's already hit the scroll
  limit, or (worse) overshoots past the thing you wanted onto something
  older. Logs its caption's duration *after* scrolling stops, using the
  real elapsed time, not a worst-case estimate.
- **Programmatic button clicks**: `cdp_locate.py --click` /
  `click_browser_element()` (`lib/session.sh`) finds the nearest ancestor
  `<button>` for a text needle and clicks it via CDP — dispatches a real
  `mousedown` → 150ms pause → `mouseup` → `click` (not a bare `.click()`),
  so the browser's native `:active` press styling actually plays. Also
  auto-overrides `window.confirm`/`alert` first, since this hand-rolled CDP
  client has no `Page.javascriptDialogOpening` handling and a real
  `confirm()` call would otherwise hang it.
- Point offsets get retuned often based on literally watching the output
  video — see `point_at_browser_text`'s own doc comment in `lib/session.sh`
  for the offset convention (+X right, +Y down; there's also a **baked-in
  `+6` baseline buffer** added below the measured text regardless of the
  caller's `OFFSET_Y`, so `OFFSET_Y=0` still lands ~6px low — cancel it with
  a negative offset if you want to land exactly on the text).

`checkerboard-calibration` (`record.sh`, `POINTING-NOTES.md`), by contrast,
**was blocked, then resolved at the `ci2` level later the same day, and
`record.sh` has since been updated to use the fix** — see
`checkerboard-calibration/POINTING-NOTES.md`'s "BLOCKED" section (top of
the file, now marked historical) for the full original writeup, and its
"Update 2026-07-20 (later the same day)" subsection right after for the
fix. Summary of the original blocker below. Two library extensions came out
of originally writing it, now available to any scenario:

**Dependency check and first real run, `strawlab` Linux dev machine
(2026-07-20):** all of `ffmpeg`/`xdotool`/`Xvfb`/`openbox`/`ttyd`/`xprop`/
`python3` plus `google-chrome` (and `firefox` as fallback) are present.
`strand-cam` is installed via the `.deb` package at `/usr/bin/strand-cam`
(`1.0.0-rc.5+c2b21b9e...`), confirmed (via `strings | grep -i "checkerboard
calibration"`) to already have `checkercal` compiled in — no rebuild
needed. `v4l2loopback` was set up via `checkerboard-calibration/
setup-v4l2loopback.sh` (handled a real DKMS multi-kernel edge case seen
here — see the diagnosis below); that script has since been deleted, once
the scenario moved to the `video-file` backend and no longer needed
`v4l2loopback` at all. A trimmed `CHECKERBOARD_VIDEO` is ready at
`checkerboard-calibration/intrinsic_cal_demo_trimmed.mp4` (120.6s,
1920x1200, video-only; the original `intrinsic_cal_demo.mp4` this was
trimmed from is no longer present in this directory as of this writing).

With all of that in place, `record.sh` still doesn't produce a video:
`nokhwa` (the `webcam` backend's underlying library) fails to open the
`v4l2loopback` device at all (`BackendError(Could not get device property
CameraFormat: Failed to Fufill)`), independent of two real `record.sh` bugs
that got found and fixed along the way (an apostrophe that broke bash's
parser, and a PATH-wrapper-vs-`open_terminal` ordering bug). This looks
like a `nokhwa`/`v4l2loopback` compatibility gap, not something fixable
from this directory — see `POINTING-NOTES.md` for the full diagnosis
(what was ruled out, what the working theory is, and why fixing
`ci2-webcam` itself was deliberately not attempted this session).

**Resolved later the same day, at the `ci2` level, not by fixing
`ci2-webcam`/`nokhwa`:** added `camera/ci2-video-file`, a new backend
(`--camera-backend video-file`) that decodes a video file directly via the
existing `media-utils/frame-source` crate — no `v4l2loopback`, `ffmpeg`
feeder process, or `nokhwa` involved at all. Purely additive (new crate +
one new `CameraBackend` enum variant/match arm in
`strand-cam/src/cli_app.rs`); no existing backend touched. Verified
directly against strand-cam with this scenario's own
`intrinsic_cal_demo_trimmed.mp4`: correct ~8.57fps native-rate playback
(matching the file's real ~120.6s duration) and zero downstream dropped
frames over a full loop cycle. See `POINTING-NOTES.md`'s "Update
2026-07-20 (later the same day)" section for two real bugs found and fixed
while verifying this. **Since done:** `record.sh` (and `README.md`'s
"Checkerboard calibration and the `video-file` backend" section) were
updated to actually use `STRAND_CAM_VIDEO_FILE`/`--camera-backend
video-file` instead of the `v4l2loopback` approach above, and
`setup-v4l2loopback.sh` was deleted — see "What's not done yet" below for
what's still outstanding (mainly: a first real end-to-end run).

- **`click_browser_element` / `cdp_locate.py --click` gained a
  configurable `--click-ancestor`** (default still `button`) — some
  widgets (e.g. this project's own `<Toggle>` component,
  `web/ads-webasm/src/components/toggle.rs`) render
  `<label><input type=checkbox></label>` with no `<button>` in their DOM at
  all; pass `ancestor_tag="label"` for those (clicking a `<label>`
  natively activates its `<input>`).
- **New `get_browser_text` / `cdp_locate.py --get-text`** reads a live
  numeric/text value out of the DOM (e.g. "Number of checkerboards
  collected: 7") — `wait_for_browser_text` can only confirm a fixed needle
  is present, not read a value that changes over time.

## Conventions this project has learned the hard way

- **Never `pkill` by process name on this machine.** It's a shared desktop
  — a name-based kill can hit the terminal hosting your own Claude Code
  session. Scope any process-killing to a specific PID/session id (see how
  `open_terminal`'s `TERM_SESSION_PID` + `pkill -s` is used in
  `braid-intro/record.sh`).
- **Verify frame extraction carefully.** Use forward-seek `ffmpeg -ss`
  timestamps (checked against `ffprobe`'s reported duration), not
  `-sseof`, before concluding something in a generated video is broken —
  `-sseof` has previously given a misleading "stuck" final frame that
  wasn't real.
- **State judgment calls explicitly and let the user correct them**, rather
  than silently picking a number and moving on — e.g. when halving an
  offset that isn't evenly divisible, or estimating a scroll-click count.
  Say what you chose and why so it's easy to redirect.
- **Don't over-verify once the user says they'll check the video
  themselves** — running `record.sh` and confirming no warnings/leftover
  processes is enough; no need to also pull frames unless something
  actually looks wrong in the script's own output.
- **Don't go spelunking through git history for "this seems lost"-type
  reports** — ask the user what they actually want restored/checked
  instead of guessing via `git log`/`git blame`.

## What's not done yet

- No full frame-by-frame comparison of `braid-intro`'s output against the
  real original `Video_2.mp4` yet (unlike `strand-cam-intro`, which got one
  — see its own `COMPARISON-NOTES.md`) — tuning so far has been iterative
  spot-feedback, not a systematic pass.
- `checkerboard-calibration/record.sh` has been run end-to-end four times
  now (see its own section above and `POINTING-NOTES.md`'s dated update
  sections) — still mid-tuning, same iterative cycle the other two
  scenarios already went through (no comparison pass yet — see
  `POINTING-NOTES.md`'s "Known gap" section). "Saved camera calibration to
  file" has never appeared in a real run so far — still unresolved, and
  (per `POINTING-NOTES.md`'s latest update) a strong candidate for the same
  scrolling-terminal-DOM root cause the end-of-video wait just got fixed
  for — worth trying the same marker-file treatment there next before
  assuming it's a genuine calibration failure.
- Git author email on old commits is still the auto-generated
  `mh1517@bio-....privat`, not the real `mh1517@bio.uni-freiburg.de` — only
  matters if this ever goes upstream.

## If picking this up cold, right now

```
cd media-utils/tutorial-video-simulation/strand-cam-intro && ./record.sh   # works anywhere
cd media-utils/tutorial-video-simulation/braid-intro && ./record.sh       # needs the real 5-camera rig
cd media-utils/tutorial-video-simulation/checkerboard-calibration && CHECKERBOARD_VIDEO=... ./record.sh  # run 4x successfully; still mid-tuning, "Saved camera calibration" wait still unresolved, see POINTING-NOTES.md
```

Watch `out/*.mp4`, get feedback, adjust the tuned constants at the top of
`record.sh` (or the shared helpers in `../lib/session.sh` /
`../lib/cdp_locate.py`), rerun. `git log --oneline -- media-utils/tutorial-video-simulation/`
has the full detailed history if you need more context than this file
gives.
