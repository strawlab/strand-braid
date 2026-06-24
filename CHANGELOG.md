## Unreleased

### Added

* The browser UIs of Braid and of standalone Strand Camera have a "Quit"
  button (with a confirmation dialog which warns when recording is active).
  Braid's quit button gracefully stops the whole system: recording is stopped
  and files are closed, all connected cameras quit, and then Braid itself
  exits. When Strand Camera runs under Braid, its own quit button is hidden;
  use Braid's.
* The Braid browser UI can now show a live preview of each connected camera,
  including detected points, without leaving the Braid UI. The camera list is a
  dense grid of per-camera tiles, each with a "live" toggle. Live tiles stream
  the camera's view through Braid's existing camera proxy; non-live tiles show
  a grayed-out placeholder and consume no camera or network resources.
* Added a `webcam` camera backend for consumer webcams (UVC and similar), using
  the operating system's native capture interface (V4L2, AVFoundation, or Media
  Foundation). It is intended as a development convenience so that Strand Camera
  can run without machine-vision hardware. Select it with `strand-cam
  --camera-backend webcam`. Webcams do not support hardware triggering, exposure,
  gain, or frame-rate limiting, so they are not suitable for synchronized
  multi-camera 3D tracking with Braid.
* Running `braid` with no command (or `braid help`) now lists the available
  `braid-<command>` subcommands discovered on the system, and an unknown command
  (e.g. a typo) prints the same list instead of a low-level "No such file or
  directory" error.
* Added `strand-cam --list-cameras`, which prints the cameras available for the
  selected `--camera-backend` (their names, models, and serials) and exits
  without launching the application or opening a browser. The printed name is
  the value to use with `--camera-name` or as a camera `name` in a Braid
  configuration file, so camera names can be discovered without launching a
  camera per terminal.
* Added `braid-sim-bench`, a reproducible scaling/timing benchmark for Braid's
  in-process 3D tracking core. It sweeps a grid of camera and insect counts,
  reports tracker throughput (frames/s and real-time factor), and writes CSV.
  Build it with
  `cargo run -p braid-sim --features inprocess --bin braid-sim-bench`. A
  companion `braid-sim-plot` binary renders the CSV into SVG scaling plots.

### Changed

* Several `strand-cam` command-line flags were renamed to standard kebab-case
  for consistency: `--mp4_filename_template`, `--fmf_filename_template`,
  `--ufmf_filename_template`, and `--force_camera_sync_mode` are now
  `--mp4-filename-template`, `--fmf-filename-template`, `--ufmf-filename-template`,
  and `--force-camera-sync-mode`. The underscore spellings are no longer
  accepted. Flags used by Braid to launch Strand Camera (`--camera-name`,
  `--braid-url`, `--camera-backend`) are unchanged.

### Fixed

* Frame-rate estimation now prefers the trigger timestamp, falling back to the
  camera's hardware (device) timestamp when no trigger is in use, and only then
  to the host clock (with a one-time warning). The trigger timestamp is derived
  from the trigger / clock model and is available whenever a trigger is
  configured, so it covers more cases than the raw device timestamp (some
  cameras expose no usable hardware timestamp). Previously the rate was always
  estimated from the host grab time, which under load is bunched (the driver
  buffers frames and the host grabs them in bursts), reading several times too
  high. A too-high frame rate makes the tracker's `dt = 1/fps` too small,
  shrinking the process noise into an overconfident motion prior that rejects
  real detections — which could drop live tracking and fragment trajectories
  (observed live as hundreds of short tracks that retracking recovered as a few
  long ones). Both the trigger and hardware timestamps reflect the true
  acquisition time and are immune to host-side buffering; the host-clock
  fallback applies when neither is available (e.g. an untriggered webcam).
* The Vimba (Allied Vision) backend now reports a clean error instead of
  aborting the process when the Vimba SDK cannot be initialized (for example
  when the SDK is not installed). When the failure is the common
  `VmbErrorNoTL` ("no transport layer") caused by `GENICAM_GENTL64_PATH` not
  pointing at the Vimba GenTL transport layers, the error includes a hint to
  finish the Vimba SDK installation (run its `Install_GenTL_Path.sh`). The
  installation docs now document this step.

## 1.0.0-rc.2 - 2026-04-26

### Added

* The support of Basler's cameras via their Pylon library is moved to a shim
  library to decouple the main codebase from the Pylon library.
* Save video to .mp4 files in Strand Camera (instead of .mkv files). Update
  Braid, `braid-process-video`, `strand-convert`, and other utilities to use
  MP4. Video is encoded with the H.264 codec and metadata, including precise
  timestamps, are stored in the h264 stream. To do compression, the [openh264
  encoder](https://github.com/cisco/openh264) is always available. With
  [appropriate NVENC
  hardware](https://developer.nvidia.com/video-encode-and-decode-gpu-support-matrix-new),
  hardware-accelerated encoding is also supported. Ffmpeg is also available when
  installed, allowing use of hardware encoding on a variety of platforms.
* Add ability for a single Strand Camera instance to perform 2D tracking in
  multiple mini arenas simultaneously. Inspired by
  [MARGO](https://github.com/de-Bivort-Lab/margo). Trajectories are confined to
  individual mini arenas. Automatic camera calibration can be performed by
  making use of April Tags embedded in the arena walls.
* Support PTP synchronized cameras without external triggering hardware. Tested
  with Basler Ace2 GigE cameras.
* Added support for Allied Vision Technologies cameras using the Vimba X
  driver. In the braid .toml configuration file, specify the camera with
  `start_backend = "vimba"`.
* Braid can now start saving MP4 files in all cameras with a single button.
  Furthermore, additional support for post-triggering of all cameras can be
  done.
* Support for [Rerun](https://www.rerun.io) to visualize data. If the
  environment variable `RERUN_VIEWER_URL` is set, Braid will attempt to connect
  to the rerun viewer at this address. A new utility (`braidz-export-rrd`)
  converts .braidz files (and, if specified, also multi-camera .mp4 videos) into
  a .rrd file for viewing with the Rerun viewer.
* Implementation of Rust-based calibration programs, eliminating the need to use
  Python flydra scripts. Updated
  https://strawlab.github.io/strand-braid/braid_calibration.html to describe the
  new tools.
* Native Rust port of
  [MultiCamSelfCal](https://github.com/strawlab/MultiCamSelfCal). Camera
  self-calibration from a `.braidz` file no longer requires Octave. The
  `braidz-mcsc` program can now run the full calibration pipeline entirely in
  Rust, including a bundle-adjustment refinement step.
* New light mode for browser UI. Selection between dark and light mode is done
  according to browser and OS preferences.
* Live view video field has new viewing modes, namely "25%", "50%" and "100%"
  scaled modes in addition to the existing "Fit" mode. Also "Rotate CW" and
  "Rotate CCW" buttons were added.
* New `burn-timestamps` utility that reads the precise timestamps stored inside
  Strand Camera MP4 files and renders them as a text overlay in a new output
  video.
* New `video2rrd` utility that converts Strand Camera MP4 video files to the
  Rerun `.rrd` format, including support for connecting directly to a running
  Rerun viewer.
* Substantial improvements to the `braid-process-video` program for processing
  saved videos and data.
* Braidz Viewer website at https://braidz.strawlab.org/ can be installed as a
  [Progressive Web App
  (PWA)](https://developer.mozilla.org/en-US/docs/Web/Progressive_web_apps/Guides/What_is_a_progressive_web_app).
  When the Braidz Viewer is installed locally, double-clicking on a `.braidz`
  file will open it in the app automatically.
* `show-timestamps` now supports SRT subtitle output (`--srt` option), making
  it easier to load frame-accurate timestamps in third-party video players.
* Strand Cam defaults to including the camera name in the saved MP4, FMF, uFMF,
  and April Tags .csv.gz files.
* Added support to save raw, uncompressed video to the MP4 container format.
* Save camera gamma to MP4 files.
* On systems with an Nvidia GPU, set the default encoding for MP4 video saving
  to H264 using NvEnc hardware.
* Provide binaries for Ubuntu 24.04 LTS Noble Numbat.
* Build for Ubuntu 22.04 (Jammy)
* Build support for Ubuntu 26.04 LTS.
* Binary release compiled with Basler Pylon version 7.3.
* Added support for building Strand Camera with Basler Pylon on aarch64 (ARM64)
  architecture.
* Security of web sessions is simplified. Braid and Strand Camera now use a
  cookie signing secret which is persisted to disk and does not require the user
  to set. A token is needed for the first request via HTTP but typically the
  token-free URL can be used for subsequent requests.
* For Strand Cam and Braid, simplify defaults so that `cargo build --release` is
  as close to just working as possible. The browser frontends still need to be
  built but an explicit compile time error is shown if this remains to be done.
* `braid` CLI now has a `help` subcommand.

### Changed

* No longer saves .mkv files. (Will now save .mp4 files instead.)
* MP4 and FMF files use frame timestamps computed from the triggerbox device if
  available.
* When saving MP4, FMF and April Tag CSV files, default filenames include the
  camera name.
* Braid no longer runs an in-process strand-cam but rather launches a child
  process for each camera. This enables support of other camera drivers and will
  enable braid to run with cameras from multiple vendors. This builds off the
  remote camera support.
* Remote cameras for braid are specified using `start_backend = "remote"` in the
  `[[cameras]]` section of the Braid `.toml` configuration file. (To update,
  replace `remote_camera = true` with `start_backend = "remote"`. The default
  setting is now `start_backend = "pylon"` to enable Basler Pylon cameras to
  continue with existing Braid `.toml` configuration files.)
* Parameter `fps` for `FakeSync` trigger mode renamed `framerate`.
* Rename command line program `offline-retrack` to `braid-offline-retrack`.
* Rename command line program `strand-cam-offline-kalmanize` to
  `flytrax-csv-to-braidz`.
* Removed `packet_capture_dump_fname` Braid configuration parameter.
* Removed the Python `flydra` submodule. All calibration functionality that
  previously depended on it is now implemented in Rust.
* GStreamer plugins (`gst-plugin-apriltag`, `gst-plugin-nvargustime`) extracted
  into their own dedicated repositories.
* Web frontend build tooling switched from `wasm-pack` to
  [`trunk`](https://trunkrs.dev/).
* Repository reorganized into thematic subdirectories: `braid/`, `geometry/`,
  `im-proc/`, `strand-cam/`, `braidz/`, `web/`, `utils/`, and `media-utils/`.
* Migrated all crates to Rust edition 2024.
* Version 1.0.0 — first major release. The jump from 0.x reflects the maturity
  and long-term stability of the software.
* Web UI frontends are now automatically built when using `cargo build` for Strand Cam and Braid. This is
  done with the `trunk` tool, which is now a build dependency. If the web UI
  has not yet been built, an explicit compile-time error is shown with instructions
  on how to build the web UI.

### Fixed

* Fixed a bug in which a remote camera could not be connected to by braid when
  launched on a remote computer.
* The `alpha` parameter in the feature detector was inadvertently ignored. This
  has been corrected. Thanks to Antoine Cribellier for noticing this.
* PTP synchronization: Strand Camera now waits for the PTP clock to converge
  before starting camera acquisition, improving timestamp accuracy in
  PTP-synchronized setups.
* When saving MP4 files, the maximum framerate parameter is respected.
* Increased the default size of the buffer used to save data to disk to 10000
  from a previous value of 10. Additionally, made this value configurable by
  creating a new parameter `write_buffer_size_num_messages` in the `[mainbrain]`
  section of the Braid `.toml` configuration file.
* `braid-process-video`: Fixed braidz output file generation, which previously
  produced incorrect output.
* Browser caching is turned off. This reduces disk usage.

## 0.11.1 - 2021-12-04

### Added

* Update build for Ubuntu 20.04 `.deb` to specify the exact Pylon version
  dependency to the package manager.

### Changed

* Update build for Ubuntu 20.04 `.deb` to use Pylon 6.2.0.

### Fixed

* Restore the checkerboard calibration to the web browser UI. (This was
  inadvertently disabled in a code reorganization.)

## 0.11.0 - 2021-12-02

### Added

* The software is released now as a single `.deb` package for Ubuntu 20.04 which
  includes the main applications (Braid and Strand Camera) in addition to
  numerous smaller utility programs.

* New program `braid-process-video` to process multiple simultaneously recorded
  MKV and FMF videos. Optionally, when a simultaneously recorded .braidz file is
  available, the data in the .braidz is used to directly indicated temporally
  synchronized frames. Without the .braidz file, an algorithm attempts to match
  frames acquired exactly synchronously, but the algorithm is imperfect.

* MKV videos now save timestamps with microsecond precision. (Previously was
  millisecond precision.)

* Support for "remote cameras" in Braid. A remote camera can be used to connect
  cameras on separate computers over the network to an instance of Braid. One or
  more instances of Strand Camera can thus run on computers other than the
  computer on which Braid is running.

* Implement loading all camera node map settings from a file. For Braid, this is
  specified in the `[[cameras]]` section with the `camera_settings_filename`
  key. For Strand Cam, set this with the `--camera-settings-filename`
  command-line argument.

* Braid stores the camera settings for each camera determined with Strand Camera
  was started into the newly created `cam_settings` directory inside the
  `.braidz` file.

* Braid stores the feature detection settings for each camera determined with
  Strand Camera was started into the newly created `feature_detect_settings`
  directory inside the `.braidz` file.

* On linux, Basler Pylon backend checks for resource limits for USB memory
  (`/sys/module/usbcore/parameters/usbfs_memory_mb`) and file descriptors
  (`ulimit`) warns if they are likely too low and suggests how to increase
  limits.

* MKV videos now save the camera name as metadata in the segment title field.

* MKV, FMF, and uFMF videos have the camera name in the filename when saved with
  Braid.

* When saving MKV videos, automatically trim pixels to fit divisible-by-two
  requirements for VPX video encoders.

* Backport to Pylon 5 for Basler cameras. This is not enabled by default but can
  be useful to debug issues dependent on driver version.

* Validate that triggerbox works in Windows.

* April Tag detection works in Windows.

* For debugging, enable ability to capture raw feature detections directly from
  Strand Cam in Braid by the new configuration parameter
  `packet_capture_dump_fname` which specifies a filename into which the data is
  stored.

* Basler cameras automatically set the stream grabber `MaxTransferSize`
  parameter to its maximum possible value. This can be disabled by setting the
  `DISABLE_SET_MAX_TRANSFER_SIZE` environment variable.

### Fixed

* Fixed computation of triggerbox pulse time and consequently estimated latency.
  There were two problems in the previous implementation, and both are now
  fixed. First, the triggerbox firmware has a bug in which two pulses are
  counted before electrical pulses were actually, physically emitted from the
  device. Secondly, the braid-triggerbox crate prior to 0.2.1 mis-calculated the
  estimated time at which the trigger pulse counter was read.

* Fixed segmentation fault that sometimes happened on program exit.

### Changed

* Removed runtime compatibility with old flydra v1 camera nodes and mainbrain.
  This had not been tested in many releases and likely didn't work anyway. This
  required some custom serialization and deserialization code, which has now
  been removed. (Braid communication between Strand Cam and the mainbrain is
  currently done with CBOR encoded data.)

## 0.10.1 - 2021-07-26

### Added

* Camera calibration for Braid can now be in the pymvg .json file format (or
  continue to remain in the flydra .xml file format).

## 0.10.0 - 2021-07-02

### Added

* Cameras automatically synchronize in `braid`.

* Braidz parsing is done by a single implementation in the `braidz-parsing`
  crate. This transparently handles `.braidz` files or `.braid` directories as
  well as uncompressed `.csv` files or compressed `.csv.gz` files.

* In `strand-cam` a camera calibration can be loaded with the
  `--camera-xml-calibration` or `--camera-pymvg-calibration` command-line
  arguments.

* Use of the human-panic crate to enable better error reporting from users.

### Changed

* For the `strand-cam-offline-kalmanize` program, the `--output` (or `-o`)
  command line argument now MUST be a filename ending with `.braidz`.
  (Previously, it was a `.braid` directory name which would implicitly get
  converted to the corresponding `.braidz` file. For example, `foo.braid` was
  given on the command line and `foo.braidz` was saved. Now, use `foo.braidz`.)

* The Kalman filter implementation used for tracking in Braid now uses the
  Joseph form method to calculate covariance. This improves robustness. See
  https://github.com/strawlab/strand-braid/issues/3.

* Error handling is now performed by the `anyhow` and `thiserror` crates in
  place of the `failure` crate.

* Asynchronous task handling was updated to `tokio` 1.0 from 0.2.

### Fixed

* When saving data with `braid`, the textlog and experiment info files are saved
  uncompressed and are flushed to disk after initial writing. Therefore, less
  data is stored in memory as the program runs. This works around a bug in
  which, if the program crashes due to a panic, some data scheduled to be
  written to a .gz file is lost.

## 0.9.1 - 2021-06-22

### Fixed

* Braid zip archive files (`.braidz` files) containing large files could become
  corrupt (https://github.com/strawlab/strand-braid/issues/5). This was fixed.

## 0.9.0 - 2021-01-04

### Added

* When braid is run without a hardware trigger box listed in the .toml config
  file, it will allow running in a "fake synchronized" mode in which most
  aspects of a triggerbox are emulated. If precise synchronization is not
  necessary, this should enable tracking without synchronization hardware. It
  also opens the door to using hardware which cannot be precisely synchronized
  such as inexpensive cameras without the ability to have an external trigger.

### Fixed

* Checkerboard calibration in strand-cam is much faster and uses sub-pixel
  corner localization. It should behave identically to the calibration in ROS
  camera_calibration.calibrator.MonoCalibrator now.

### Changed

* The log output format has changed slightly as we are now using the
  [tracing](https://crates.io/crates/tracing) library for logging.
* Migrate some error handling to the `thiserror` and `anyhow` crates (from the
  `failure` crate). To continue to provide tracebacks when the RUST_BACKTRACE
  environment variable is set, binaries for release are build with rust nightly
  from 2021-01-03.
* Several of the internal crates have been individually licensed under
  Apache-2.0/MIT or similar licenses.

## 0.8.2 - 2020-12-04

### Fixed

* Support Basler Ace2 cameras

## 0.8.1 - 2020-11-15

### Fixed

* force MKV frame width to be power of 2 when saving in strand-cam.
* fix braidz parser not to fail on "unknown" fps.
* revert to use NVENCAPI_VERSION 8 to support older drivers.

## 0.8.0 - 2020-11-03

### Added

* Update to Pylon 6.1.1
* Raise error dialog in strand-cam browser UI if frame processing is falling behind frame production
* Rewritten tracking core. (Numerically it produces similar or identical results, but the code is much better organized for future updates.)
* Several small feature and bug fixes.

### Changed

* For encoding h264 video, use NVENCAPI_VERSION 11, which is recent from nvidia.

## 0.7.4 - 2020-03-01

### Added

* [strand-cam] Support online detection of April Tags and saving results
  to .csv file.

* [strand-cam] Shutdown nicely when receiving SIGTERM (in addition to previous support
  for shutdown on SIGINT when Ctrl-C happens)

* [braid] Shutdown nicely when receiving SIGTERM (in addition to previous support
  for shutdown on SIGINT when Ctrl-C happens)

* Added new demo script [`./scripts/change-tracking-settings-demo.py`](./scripts/change-tracking-settings-demo.py).

* Many internal libraries updated to latest release to support rust async. This
  includes updating to tokio 0.2 and hyper 0.13. No regressions have been seen
  in testing.

### Changed

* [strand-cam] .zip file with .deb packages is renamed to
  `rust-cam-xenial-debs-build-${CI_COMMIT_SHA}`

### Note on version numbers

Version 0.7.3 was not publicly released but was internally released on
2019-12-02. It differed from the 0.7.2 release only in that it allows disabling
the Kalman filter based tracking in the camtrig variant of strand-cam

## 0.7.2 - 2019-12-02

### Added

* [strand-cam] Print backtraces for some errors, even without RUST_BACKTRACE
  environment variable being set.

* [braidz.strawlab.org] Link to main braid page at https://strawlab.org/braid

### Fixed

* [strand-cam] Shutdown nicely when receiving Ctrl-C.

* [braid] Shutdown nicely when receiving Ctrl-C, including finishing saving
  of .braid directory into .braidz file.

* [braid] Fix UI to blink when saving .braidz file. Fix UI to show ".braidz
  file" (instead of ".braid directory").

* [braidz.strawlab.org,compute-flydra1-compat] Make .braidz parsing more robust.
  In particular, if a recording was terminated abruptly, the internal CSV files
  may have an error in the final row. In that case, now we skip the final row
  rather than returning an error.

## 0.7.1 - 2019-11-25

### Changed

 * [strand-cam] change Event Source URL to `/strand-cam-events` (changed from
   `/strand-camevents`, which was a typo. Originally this was `/fview2-events`).
   The event name is also changed to `strand-cam` (changed from `bui_backend`).

 * [braid] change Event Source URL to `/braid-events` (changed from
   `/events`). Note that this does not affect the realtime pose events, which
   are at a different URL and remain at `/events`. The event name is also
   changed to `braid` (changed from `bui_backend`).

### Added

* [strand-cam, braid] In UI page, title and info link to Straw Lab website. A
  "loading..." indication is shown prior to main UI being loaded.

* [braid] Allow setting uuid in experiment_info table via HTTP call.

* Decreased logging level for many messages to reduce console spam.

### Fixed

* [strand-cam] Do not print information about how to workaround a VLC bug by
  copying the h264 stream using ffmpeg. We discovered that this will lose
  precise timestamps and so it is dangerous and should not be done.

* [braid] Do not crash when attempting 3D tracking and some cameras are not in
  the calibration. (The data from these cameras will simply not contribute to
  3d tracking.)

* [braid] Prevent occasional crash with the involving triggerbox_comms thread.

### Note on version numbers

Version 0.7.0 was not publicly released but was internally released on
2019-11-21. Internal testing revealed bugs that were fixed before the 0.7.1
release.

## 0.6.0 - 2019-10-25

### Fixed

* [braid] Fixed some a bug in which Braid would crash due to a
  `NotDefinitePositive` error when doing 3D tracking.

## 0.5.0 - 2019-10-22

### Added

* [strand-cam] On systems with NVIDIA graphics cards, enable recording to H264
  encoded MKV files using hardware encoding, thus using hardly any CPU
  resources.

# ------------------------------------------------------------------------

## unreleased

### Fixed

* Do not draw an orientation in web browser when no theta detected.

### Added

* Added `Polygon` to possible `valid_region` types.
* Updated to use libvpx 1.8 for encoding MKV videos.
* Add checkerboard calibration within fview2

### Changed

* `flydra2` now saves all output as `.csv.gz` (not `.csv`) files.

## 0.20.29 - 2019-06-06

### Changed

* For `fview2` (all variants), build with jemalloc memory allocator. This
  appears to fix a "corrupted size vs. prev_size" error.

## 0.20.28 - 2019-06-01

### Added

* Created several ROS launch example files. They are in this repository in the
  `ros-launchfile-examples` directory.

### Fixed

* Fixed some bugs in the way .mkv files were created. There was a bug in which
  recordings longer than ~30 minutes were truncated at ~30 minutes. And another
  bug was that, when viewing the recorded video, skipping to a particular point
  in time and viewing the total duration did not work. With some light testing,
  these should both be fixed now.
* Fixed setting of acquisition frame rate on older GigE cameras.
* Provide mime type for .js files in fview, which stops browser warning about
  empty mime type.

### Changed

* For `fview2` (all variants, Pylon drivers), upgrade Pylon to version
  5.2.0.13457.
* In `flydra2-mainbrain`, changed the `--addr` command-line argument to
  `--lowlatency-camdata-udp-addr`.

## 0.20.25 - 2019-03-25

### Changed

* In `fview2` (all variants), CSV files from object detection have a new, more
  efficient format.

### Added

* In `fview2` (all variants), the maximum framerate for saving data to CSV files
  may be specified.

## 0.20.22 - 2019-02-24

### Changed

* In `fview2` (all variants, Pylon drivers), if a Pylon camera is already open,
  open a web browser to a guess of the correct URL. Disable with `--no-browser`.

## 0.20.21 - 2019-02-24

### Changed

* Substantial browser UI revision in `fview2` (all variants).

* `fview2-camtrig` will raise an error dialog in browser UI if contact
  to the camtrig USB device is lost.

* Default codec for MKV files is VP8 in `fview2` (all variants). This was a
  change from VP9 because there is seems to be a bug when saving to VP9 in which
  the encoded video is jerky.
