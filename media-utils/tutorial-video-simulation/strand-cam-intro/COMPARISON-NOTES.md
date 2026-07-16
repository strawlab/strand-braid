# strand-cam-intro vs. the original Video_1.mp4

Notes from a frame-by-frame comparison (2026-07-16) between
`strand-cam-intro/record.sh`'s output and the real original this tool
regenerates:

```
/mnt/strawscience/mjmharrap/Braid_for_dummies/Supplemental_or_data_items/Video_1.mp4
```

(1920x1200, ~73s, recorded on this same machine's real GNOME desktop,
`strawlab@apis`, showing the now-removed `strand-cam-pylon` binary against a
real Basler camera.)

Ordered roughly by visual impact.

## 1. Window layout -- biggest remaining gap (DONE 2026-07-16)

The original uses normal floating windows with visible desktop margin
around them: the terminal is roughly 42% of screen width, centered-left
with space above/below/around it; the browser is a separate floating
window to its right, a different height than the terminal (taller, since
its content is taller). Ours tiles both windows edge-to-edge, filling the
entire screen with zero margin -- that wall-to-wall look is probably the
single biggest remaining tell that it's not a real desktop, even with the
purple background blending in.

**Fix applied:** `lib/session.sh` now has `SESSION_MARGIN` (48px) and
computes `SESSION_PANE_WIDTH`/`SESSION_PANE_HEIGHT` accordingly;
`open_terminal`/`open_browser` position/size both windows with that margin
around and between them instead of exact zero-gap half-screen splits.
Verified visually -- background is now visible around and between both
windows.

## 2. Window decorations (accepted gap, not pursuing per earlier decision)

The original has GNOME's rounded title bar with search/hamburger icons;
ours is plain `xterm`'s rectangular bar. This is the residual gap from an
earlier explicit call ("theme colors only, not full GNOME desktop
replication" -- full GNOME Shell replication inside Xvfb was judged not
worth the effort/risk). Just flagging it's the most visible thing left if
that decision ever gets revisited.

## 3. Copy-paste camera name (optional, low priority)

Before typing the second command, the original double-clicks to select the
camera name (`Basler-40311076`) directly out of the log output, then
presumably pastes it into `--camera-name `, rather than retyping it from
memory. Since our sim camera names are short (`simcam0`), retyping isn't
really wrong -- just noting the original's workflow was "copy from log," not
"type from memory." Could add `xdotool` double-click + copy/paste if closer
fidelity is ever wanted, but low value for a 7-character name.

## 4. Second browser tab (intentionally NOT matching)

On relaunch, the original opens a genuinely new browser tab (2 tabs visible
by the end, both titled "Basler-40311076 - Strand Cam"). Ours reconnects
the same single tab via the frontend's own auto-reconnect logic. The
original's second tab is really just an artifact of `strand-cam` calling
the system's default browser opener on every launch, not something
meaningful to the tutorial -- recommend keeping our current single-tab
behavior rather than chasing this.

## 5. Recording resolution (low priority)

Original is 1920x1200; ours is 1280x800. Same-ish aspect ratio (1.6:1 vs
1.6:1), so low priority to change.

## 6. Realistic Basler-style camera names (deferred, not pursuing for now)

Requested: rename the sim cameras from `simcam0..simcam4` to look like real
Basler serials (`Basler-40290626`, `Basler-40423939`, `Basler-40311076`,
`Basler-40454395`, `Basler-40290624`), matching the original's real camera
name. Investigated 2026-07-16: this is bigger than a tutorial-scoped change.

- `simcam{k}` naming is hardcoded in `braid-sim`'s core library
  (`Scenario::camera_name`/`camera_index`, `braid/braid-sim/src/scenario.rs:484-491`),
  called from ~15+ sites across the `ci2-sim` and `braid-sim` crates
  (calibration, projection, harness, benchmarks) -- not something
  `example-sim.toml` or our tutorial's config can override today.
- A doc comment on `camera_name` explicitly says names are "kept purely
  alphanumeric to avoid ROS-name encoding mismatches" -- a hyphenated name
  like `Basler-40290626` is exactly what that's warding off, somewhere
  downstream in ROS-facing code not fully traced.
- Several existing tests hardcode `simcam0`/`simcam1`/etc.
  (`ci2-sim/tests/pipeline.rs`, `ci2-sim/tests/timestamps.rs`,
  `braid-sim/tests/harness.rs`, `braid-sim/tests/core.rs`) plus two
  smoke-test scripts.
- Feasible path if revisited: add an opt-in `names: Option<Vec<String>>`
  field to `CameraRig` (defaulting to `None`, preserving today's `simcam{k}`
  behavior everywhere/for everyone else), turn `camera_name`/`camera_index`
  from free functions into scenario-aware methods that consult it, and give
  our tutorial its *own* separate sim-config file with custom names rather
  than touching the shared `example-sim.toml` -- fully additive, wouldn't
  break the tests above. Could also sidestep the ROS-naming concern by
  dropping the hyphen (`Basler40290626`) while still looking very close to
  a real serial.
- User's call: "don't change this now" -- deferred, `simcam0..4` stays as
  the accepted/out-of-scope difference it always was.

## Not applicable / already understood

- The original's recording tool shows its own "Start/Pause/Cancel/Save
  recording" menu popping up at the very start and end -- an artifact of
  however they captured the original, not tutorial content.
- The big yellow mouse-icon-with-key-badge overlay in the original is that
  same recording tool's keypress visualizer, not real cursor movement --
  already addressed by adding real `xdotool mousemove` calls instead (see
  git history / project memory for that change).
- Live-view content differences (real Basler camera footage vs. our
  simulated black canvas with a moving dot) are expected and out of scope
  -- the user explicitly said to ignore strand-cam's own on-screen content
  differences.
