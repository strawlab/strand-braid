## 0.10.0 - unreleased

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
