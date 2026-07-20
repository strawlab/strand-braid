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

## Two scenarios, very different hardware requirements

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

## Current state (as of commit `402ea8f2`, 2026-07-20)

Both scripts run end-to-end cleanly against real hardware, zero leftover
processes. `braid-intro` went through many rounds of "run it, watch the
generated video, get specific feedback, fix, rerun" this session. Notable
fixes worth knowing about if you're touching this code again:

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
- Git author email on old commits is still the auto-generated
  `mh1517@bio-....privat`, not the real `mh1517@bio.uni-freiburg.de` — only
  matters if this ever goes upstream.

## If picking this up cold, right now

```
cd media-utils/tutorial-video-simulation/strand-cam-intro && ./record.sh   # works anywhere
cd media-utils/tutorial-video-simulation/braid-intro && ./record.sh       # needs the real 5-camera rig
```

Watch `out/*.mp4`, get feedback, adjust the tuned constants at the top of
`record.sh` (or the shared helpers in `../lib/session.sh` /
`../lib/cdp_locate.py`), rerun. `git log --oneline -- media-utils/tutorial-video-simulation/`
has the full detailed history if you need more context than this file
gives.
