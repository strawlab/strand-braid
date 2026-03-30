# Strand-Braid

[![User's Guide](https://img.shields.io/badge/docs-User's%20Guide-blue.svg?logo=Gitbook)](https://strawlab.github.io/strand-braid/)

## Description

[Strand Camera](https://strawlab.org/strand-cam/) is low-latency camera
acquisition and tracking software. It is useful for 2D tracking of animals,
robots, or other moving objects. It also serves as the basis for 3D tracking
using Braid.

[Braid](https://strawlab.org/braid/) is multi-camera acquisition and tracking
software. It is useful for 3D tracking of animals, robots, or other moving
objects. It operates with low latency and is suitable for closed-loop
experimental systems such as [virtual reality for freely moving
animals](https://strawlab.org/freemovr/).

This repository is a mono repository that houses the source code for both pieces
of software as well as many other related pieces, mostly written as Rust crates.

Users, as opposed to developers, of this software should refer to the
[strand-braid-user directory](strand-braid-user) which contains user
documentation and scripts for interacting with the software and performing data
analysis.

## Documentation

* [User's Guide](https://strawlab.github.io/strand-braid/)

## Discussion

* [Google Group: multi-camera software from the Straw Lab](https://groups.google.com/g/multicams)

## Citation

While a new publication specifically about Braid should be written, in the
meantime, please cite the following paper about the predecessor to Braid:

* Straw AD, Branson K, Neumann TR, Dickinson MH. Multicamera Realtime 3D
  Tracking of Multiple Flying Animals. *Journal of The Royal Society Interface
  8*(11), 395-409 (2011)
  [doi:10.1098/rsif.2010.0230](https://dx.doi.org/10.1098/rsif.2010.0230)

If you additionally make use of 3D tracking of objects under water with cameras
above water (i.e. perform fish tracking), please additionally cite this:

* Stowers JR*, Hofbauer M*, Bastien R, Griessner J⁑, Higgins P⁑, Farooqui S⁑,
  Fischer RM, Nowikovsky K, Haubensak W, Couzin ID,    Tessmar-Raible K✎, Straw
  AD✎. Virtual Reality for Freely Moving Animals. *Nature Methods 14*, 995–1002
  (2017) [doi:10.1038/nmeth.4399](https://dx.doi.org/10.1038/nmeth.4399)

## Installing

Please see the [Installation section of our User
Guide](https://strawlab.github.io/strand-braid/installation.html).

## Building for Development

### Prerequisites

[Install rust](https://rustup.rs/).

[Install trunk](https://trunkrs.dev/#install)

Install your camera drivers. Currently Basler Pylon and Allied Vision Vimba are
supported.

First checkout the git repository into a location which will below be called
`/path/to/strand-braid`:

```
cd /path/to # <---- change this to a suitable filesystem directory
git clone https://github.com/strawlab/strand-braid
cd strand-braid # now in /path/to/strand-braid
```

### Strand Camera

First, build the browser user interface (BUI) for Strand Camera. This will build
files in `strand-cam/yew_frontend/dist` which get included in the Strand Cam
executable:

```
cd /path/to/strand-braid/strand-cam/yew_frontend
trunk build
```

Then, build the Strand Cam executable for Basler cameras using the Pylon
drivers, which must be preinstalled:

```
cd /path/to/strand-braid/strand-cam/strand-cam-pylon
cargo build --release
# By default, the executable will be put in /path/to/strand-braid/target/release/strand-cam-pylon
```

Alternatively or additionally, build the Strand Cam executable for Allied Vision
cameras using the Vimba drivers, which must be preinstalled:

```
cd /path/to/strand-braid/strand-cam/strand-cam-vimba
cargo build --release
# By default, the executable will be put in /path/to/strand-braid/target/release/strand-cam-vimba
```

Many compile-time options exist to adjust the exact features used, but the
instructions above should build a working copy of Strand Camera albeit with
potentially reduced features and performance.

### Braid

We will build `braid-run` which is the main runtime application we call "Braid".

First, build the browser user interface (BUI) for Braid. This will build files
in `braid/braid-run/braid_frontend/dist` which get included in the `braid-run`
executable:

```
cd /path/to/strand-braid/braid/braid-run/braid_frontend
./build.sh
```

Then, build the `braid-run` executable:

```
cd /path/to/strand-braid/braid/braid-run
cargo build --release
# By default, the executable will be put in /path/to/strand-braid/target/release/braid-run
```


## License

This software is developed by Prof. Dr. Andrew Straw at the University of
Freiburg, Germany.

This open-source software is distributable under the terms of the Affero General
Public License v1.0 only. See [COPYRIGHT](COPYRIGHT) and
[LICENSE.txt](LICENSE.txt) for more details.

## Future license plans

We have a goal to release many of the generally useful crates under licenses
such as the MIT license, the Apache License (Version 2.0), and BSD-like
licenses. Please get in touch if there are specific pieces of code where this
would be helpful so we can judge interest and prioritize this.

## Contributions

Any kinds of contributions are welcome as a pull request.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this software by you, as defined in the Apache-2.0 license,
shall be dual licensed under the terms of the

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

without any additional terms or conditions. (This helps us realize the future
license plans as described above.)

## Code of conduct

Anyone who interacts with this software in any space, including but not limited
to this GitHub repository, must follow our [code of
conduct](code_of_conduct.md).

## Repository organization

<!-- workspace-docs start -->
<!-- This section is automatically generated by the `workspace-docs` crate. -->
### `braid/` - multi-camera realtime 3D tracker

<details>

 - braid (braid) - multi-camera realtime 3D tracker
 - braid-types (braid/braid-types) - 
 - braid-run (braid/braid-run) - run Braid, the multi-camera realtime 3D tracker
 - braidz-writer (braid/braidz-writer) - 
 - braid_frontend (braid/braid-run/braid_frontend) - 
 - braidz-writer-cli (braid/braidz-writer/cli) - 
</details>

### `strand-cam/` - single camera application

<details>

 - flytrax-io (strand-cam/flytrax-io) - 
 - strand-cam (strand-cam) - 
 - strand-cam-offline-checkerboards (strand-cam/strand-cam-offline-checkerboards) - Generate camera intrinsic camera calibration from directory full of images
 - strand-cam-pylon (strand-cam/strand-cam-pylon) - 
 - strand-cam-pylon-gui (strand-cam/strand-cam-pylon-gui) - 
 - strand-cam-vimba (strand-cam/strand-cam-vimba) - 
 - strand-cam-frontend-yew (strand-cam/yew_frontend) - 
</details>

### `camera/` - camera drivers

<details>

 - strand-cam-remote-control (camera/strand-cam-remote-control) - Types for Strand Camera remote control and configuration
 - strand-cam-types (camera/strand-cam-types) - Core types for camera control and configuration in the Strand Camera ecosystem
 - ci2 (camera/ci2) - 
 - ci2-async (camera/ci2-async) - 
 - ci2-cli (camera/ci2-cli) - 
 - ci2-pyloncxx (camera/ci2-pyloncxx) - 
 - ci2-pylon-types (camera/ci2-pylon-types) - 
 - ci2-simple-async-demo (camera/ci2-simple-async-demo) - 
 - ci2-vimba (camera/ci2-vimba) - 
 - ci2-vimba-types (camera/ci2-vimba-types) - 
 - vimba (camera/vimba) - 
 - ci2-simple-demo (camera/ci2-simple-demo) - 
</details>

### `media-utils/` - video I/O and image processing

<details>

 - apriltag-detection-writer (media-utils/apriltag-detection-writer) - 
 - y4m-writer (media-utils/y4m-writer) - 
 - ufmf (media-utils/ufmf) - 
 - fmf (media-utils/fmf) - 
 - frame-source (media-utils/frame-source) - 
 - mkv-strand-reader (media-utils/mkv-strand-reader) - 
 - mkv-parser-kit (media-utils/mkv-parser-kit) - Library for building Matroska (MKV) file parsers
 - mp4-writer (media-utils/mp4-writer) - 
 - less-avc-wrapper (media-utils/less-avc-wrapper) - 
 - font-drawing (media-utils/font-drawing) - 
 - bg-movie-writer (media-utils/bg-movie-writer) - 
 - ffmpeg-rewriter (media-utils/ffmpeg-rewriter) - 
 - ffmpeg-writer (media-utils/ffmpeg-writer) - 
 - srt-writer (media-utils/srt-writer) - 
 - burn-timestamps (media-utils/burn-timestamps) - 
 - create-timelapse (media-utils/create-timelapse) - create timelapse video from mp4 h264 source without transcoding
 - dump-frame (media-utils/dump-frame) - 
 - fmf-cli (media-utils/fmf/fmf-cli) - work with .fmf (fly movie format) files
 - show-timestamps (media-utils/show-timestamps) - 
 - strand-convert (media-utils/strand-convert) - 
 - tiff-decoder (media-utils/tiff-decoder) - 
 - video2rrd (media-utils/video2rrd) - Convert video with Strand Cam timestamps to RRD format for Rerun Viewer
</details>

### `geometry/` - camera geometry, calibration, and 3D math

<details>

 - simple-obj-parse (geometry/simple-obj-parse) - 
 - textured-tri-mesh (geometry/textured-tri-mesh) - 
 - braid-mvg (geometry/braid-mvg) - Braid's camera geometry and multi-view geometry (MVG) types and algorithms.
 - flydra-mvg (geometry/flydra-mvg) - 
 - refraction (geometry/refraction) - 
 - bisection-search (geometry/refraction/bisection-search) - 
 - braid-apriltag-types (geometry/braid-apriltag-types) - 
 - parry-geom (geometry/parry-geom) - 
 - undistort-image (geometry/undistort-image) - 
 - flytrax-apriltags-calibration (geometry/braid-april-cal/flytrax-apriltags-calibration) - 
 - braid-april-cal (geometry/braid-april-cal) - 
 - camcal (geometry/camcal) - 
 - opencv-calibrate (geometry/opencv-calibrate) - 
 - braid-april-cal-cli (geometry/braid-april-cal/braid-april-cal-cli) - Create a multi-camera calibration using known intrinsics and the SQPnP algorithm
 - bundle-adj (geometry/bundle-adj) - 
 - braid-april-cal-webapp (geometry/braid-april-cal/braid-april-cal-webapp) - 
 - mvg-util (geometry/braid-mvg/mvg-util) - 
 - braidz-mcsc (geometry/braidz-mcsc) - 
 - mcsc-structs (geometry/mcsc-structs) - 
 - find-chessboard (geometry/opencv-calibrate/find-chessboard) - find chessboard corners in an input image
</details>

### `utils/` - general-purpose utilities

<details>

 - download-verify (utils/download-verify) - 
 - strand-cam-enum-iter (utils/strand-cam-enum-iter) - A utility crate to provide an EnumIter trait for iterating over enums in the Strand Camera ecosystem
 - strand-datetime-conversion (utils/strand-datetime-conversion) - Convert between chrono and f64 time. Used in Strand Camera and Braid.
 - strand-withkey (utils/strand-withkey) - defines the WithKey trait for Strand Camera
 - env-tracing-logger (utils/env-tracing-logger) - 
 - csv-eof (utils/csv-eof) - 
 - groupby (utils/groupby) - 
 - zip-or-dir (utils/zip-or-dir) - read files from either a zip file or a directory
 - env-tracing-logger-sample (utils/env-tracing-logger/env-tracing-logger-sample) - 
 - workspace-docs (utils/workspace-docs) - CLI program to maintain repository overview in workspace README.md
 - write-debian-changelog (utils/write-debian-changelog) - simple and hacky CLI program to print a debian changelog
 - dir2zip (utils/zip-or-dir/dir2zip) - CLI program to convert a directory to a zip file
</details>

### uncategorized / miscellaneous

<details>

 - ads-apriltag (ads-apriltag) - 
 - apriltag-track-movie (ads-apriltag/apriltag-track-movie) - use ffmpeg to decode input movie and output csv file with april tag detections
 - ads-webasm (ads-webasm) - 
 - ads-webasm-example (ads-webasm/example) - 
 - braid-config-data (braid-config-data) - 
 - braid-http-session (braid-http-session) - 
 - braid-offline (braid-offline) - 
 - braid-process-video (braid-process-video) - process videos within the Braid multi-camera framework
 - braidz-parser (braidz-parser) - 
 - braidz-chunked-iter (braidz-parser/braidz-chunked-iter) - 
 - pybraidz-chunked-iter (braidz-parser/braidz-chunked-iter/pybraidz-chunked-iter) - 
 - braidz-cli (braidz-parser/braidz-cli) - 
 - braidz-rerun (braidz-rerun) - 
 - braidz-export-rrd (braidz-rerun/braidz-export-rrd) - 
 - rerun-braidz-viewer (braidz-rerun/rerun-braidz-viewer) - 
 - braidz-types (braidz-types) - 
 - braidz-viewer (braidz-viewer) - 
 - build-util (build-util) - 
 - event-stream-types (event-stream-types) - 
 - fastfreeimage (fastfreeimage) - 
 - flydra-feature-detector-types (flydra-feature-detector/flydra-feature-detector-types) - Configuration types for Strand Camera, Braid and Flydra feature detection.
 - flydra-pt-detect-cfg (flydra-feature-detector/flydra-pt-detect-cfg) - Default values for the flydra-feature-detector-types crate
 - flydra-feature-detector (flydra-feature-detector) - 
 - flydra2 (flydra2) - 
 - flytrax-csv-to-braidz (flytrax-csv-to-braidz) - 
 - freemovr-calibration (freemovr-calibration) - 
 - ncollide-geom (freemovr-calibration/ncollide-geom) - 
 - freemovr-calibration-cli (freemovr-calibration/freemovr-calibration-cli) - 
 - freemovr-calibration-webapp (freemovr-calibration/freemovr-calibration-webapp) - 
 - imops (imops) - 
 - strand-led-box-comms (led-box/strand-led-box-comms) - Communication protocol types for the Strand Camera LED Box device.
 - led-box (led-box/led-box) - 
 - led-box-standalone (led-box/led-box-standalone) - 
 - dynlink-cuda (nvenc/dynlink-cuda) - 
 - dynlink-nvidia-encode (nvenc/dynlink-nvidia-encode) - 
 - nvenc (nvenc) - 
 - gen-nvenc-bindings (nvenc/dynlink-nvidia-encode/gen-nvenc-bindings) - 
 - strand-bui-backend-session-types (strand-bui-backend-session/types) - Types for Strand Camera BUI (Browser User Interface) backend session management
 - strand-bui-backend-session (strand-bui-backend-session) - Backend session management for the BUI (Browser User Interface) used by Strand Camera and Braid
 - strand-cam-bui-types (strand-cam-bui-types) - Type definitions for the Strand Camera Browser User Interface (BUI) system.
 - strand-cam-csv-config-types (strand-cam-csv-config-types) - 
 - strand-cam-pseudo-cal (strand-cam-pseudo-cal) - 
 - strand-cam-storetype (strand-cam-storetype) - Type definitions for Strand Camera's state management and browser UI communication.
 - strand-dynamic-frame (strand-dynamic-frame) - images from machine vision cameras used in Strand Camera
 - strand-http-video-streaming-types (strand-http-video-streaming/strand-http-video-streaming-types) - Type definitions for HTTP video streaming functionality in the Strand Camera ecosystem.
 - strand-http-video-streaming (strand-http-video-streaming) - 
 - tracking (tracking) - 
</details>

<!-- workspace-docs end -->
