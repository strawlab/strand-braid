# fview2 change log

## unreleased

### Changed

* camnode2: ROS topic `/<cam_name>/camera_info` is latched.

## [0.1.15] - 2018-05-05

### Changed

* Restore inadvertently changed default to track points using absolute
  difference and everywhere in image.

## [0.1.14] - 2018-04-26

### Fixed

* camnode2: restore `/<cam_name>/set_camera_info` service necessary for
  saving the radial distortion calibrations.

### Changed

* Update browser user interface to use yew (not elm) and simple CSS (not MDL)

## [0.1.13] - 2018-02-14

### Changed

* Update expiry date to 2018-06-01

## [0.1.12] - 2018-02-14

* Update to rosrust 0.6 in camnode2.
* Change priority of some log messages.

## [0.1.11] - 2018-01-12

### Added

* Added command-line argument `--tracker-config FILENAME` where FILENAME is a YAML
  file containing the tracker configuration which could previously only be set
  in the browser interface.
* Added command-line argument `--sched-policy` and `--sched-priority` to set the
  POSIX scheduler policy. (On linux `--sched-policy 1 --sched-priority 99` would
  be `SCHED_FIFO` max priority.)

### Fixed

* Fix error during startup when sending object detections fails with "resource
  temporarily unavailable". This resulted from a non-blocking socket `send()`
  call when the listener wasn't `recv()`ing the data and thus the kernel buffer
  filled up. Now, the packet is simply dropped and a warning issued.
* Due to the above, there is no longer a need for the `--ros-tracking-start-delay`
  command-line argument and object detection begins immediately in the ROS
  version of fview2.
* Close the camera nicely when quitting with Ctrl-C.

### Changed

* On linux, do not run with SCHED_FIFO max priority by default (see new CLI args
  above). Removed `RUSTCAM_MAX_PRIORITY` environment variable.

## [0.1.10] - 2017-12-17

### Changed

* On linux, runs with scheduler set to SCHED_FIFO max priority.

## [0.1.9] - 2017-12-17

### Fixed

* Frame grabs resulting in `Failed` do not stop the program with an error. Instead,
  just the single frame is dropped.
* On linux, the Pylon .so libraries from Basler are shipped with the built .deb files
  for `fview2-pylon`. This removes the dependency on the `pylon` debian package from
  Debian and eliminates the requirement that `fview2-pylon` was built with the
  installed version of `pylon`. The `camnode2-pylon` package depends on `fview2-pylon`
  package and uses .so files installed as part of `fview2-pylon`.

### Added

* The `--version` CLI arg will also report the backend name and version, if available.
* `RUSTCAM_PYLON_ENABLE_RESEND` environment variable, when set to 0, disables packet
  resending on GigE cameras.
* `RUSTCAM_PYLON_PACKET_SIZE` environment variable allows setting the GigE packet size.

### Changed

* On linux, the file `/lib/udev/rules.d/70-fview2-pylon.rules` is installed by
  the .deb files so that the Basler `pylon` package need not be installed.
