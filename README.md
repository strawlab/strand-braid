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
 - braid-config-data (braid/braid-config-data) - type definitions for Braid configuration
 - braid-offline (braid/braid-offline) - offline Braid-based tracking
 - braid-process-video (braid/braid-process-video) - process videos within the Braid multi-camera framework
 - braid-run (braid/braid-run) - run Braid, the multi-camera realtime 3D tracker
 - braid-types (braid/braid-types) - core type definitions for the Braid multi-camera tracking system
 - braid_frontend (braid/braid-run/braid_frontend) - Yew WASM frontend for the Braid multi-camera tracking web UI
 - braidz-writer (braid/braidz-writer) - write .braidz data files for the Braid tracking system
 - braidz-writer-cli (braid/braidz-writer/cli) - CLI tool to convert a Braid data directory into a .braidz file
 - flydra2 (braid/flydra2) - high-level multi-camera 3D tracking, ported from Flydra
 - tracking (braid/tracking) - motion and observation models for Kalman filtering
</details>

### `braidz/` - braidz file handling

<details>

 - braidz-chunked-iter (braidz/braidz-parser/braidz-chunked-iter) - iterate over Braid tracking data in time-based or frame-based chunks
 - braidz-cli (braidz/braidz-parser/braidz-cli) - CLI tool to inspect and query .braidz tracking data files
 - braidz-export-rrd (braidz/braidz-rerun/braidz-export-rrd) - CLI program to export a rerun .rrd file from a .braidz file
 - braidz-parser (braidz/braidz-parser) - parse .braidz files from the Braid multi-camera tracking system
 - braidz-rerun (braidz/braidz-rerun) - export data in braidz files to rerun
 - braidz-types (braidz/braidz-types) - core type definitions for braidz files
 - braidz-viewer (braidz/braidz-viewer) - web based quick .braidz viewer
 - flytrax-csv-to-braidz (braidz/flytrax-csv-to-braidz) - convert .csv file saved by flytrax in strand-cam to .braidz format
 - pybraidz-chunked-iter (braidz/braidz-parser/braidz-chunked-iter/pybraidz-chunked-iter) - Python bindings for braidz-chunked-iter
 - rerun-braidz-viewer (braidz/braidz-rerun/rerun-braidz-viewer) - a build of the rerun viewer which can directly visualize .braidz files
</details>

### `strand-cam/` - single camera application

<details>

 - strand-cam (strand-cam) - Strand Camera: a single-camera recording and realtime tracking application
 - strand-cam-csv-config-types (strand-cam/strand-cam-csv-config-types) - support YAML frontmatter in .csv files saved by Strand Camera
 - strand-cam-frontend-yew (strand-cam/yew_frontend) - Yew WASM frontend for the Strand Camera web user interface
 - strand-cam-offline-checkerboards (strand-cam/strand-cam-offline-checkerboards) - Generate camera intrinsic camera calibration from directory full of images
 - strand-cam-pseudo-cal (strand-cam/strand-cam-pseudo-cal) - create camera calibration for Braid using only a view of a circle
 - strand-cam-pylon (strand-cam/strand-cam-pylon) - Strand Camera binary using the Basler Pylon camera backend
 - strand-cam-pylon-gui (strand-cam/strand-cam-pylon-gui) - Strand Camera with Basler Pylon backend and eframe GUI
 - strand-cam-storetype (strand-cam/strand-cam-storetype) - Type definitions for Strand Camera's state management and browser UI communication.
 - strand-cam-vimba (strand-cam/strand-cam-vimba) - Strand Camera binary using the Allied Vision Vimba camera backend
</details>

### `camera/` - camera drivers

<details>

 - ci2 (camera/ci2) - camera interface (ci2) trait definitions for machine vision cameras
 - ci2-async (camera/ci2-async) - asynchronous wrapper for the ci2 camera interface
 - ci2-cli (camera/ci2-cli) - command-line interface for ci2 camera backends
 - ci2-pylon-types (camera/ci2-pylon-types) - Pylon-specific type definitions for the ci2 camera interface
 - ci2-pyloncxx (camera/ci2-pyloncxx) - ci2 camera backend using the Basler Pylon SDK
 - ci2-simple-async-demo (camera/ci2-simple-async-demo) - simple demonstration of the ci2 asynchronous camera API
 - ci2-simple-demo (camera/ci2-simple-demo) - simple demonstration of the ci2 camera API
 - ci2-vimba (camera/ci2-vimba) - ci2 camera backend using the Allied Vision Vimba SDK
 - ci2-vimba-types (camera/ci2-vimba-types) - Vimba-specific type definitions for the ci2 camera interface
 - strand-cam-remote-control (camera/strand-cam-remote-control) - Types for Strand Camera remote control and configuration
 - strand-cam-types (camera/strand-cam-types) - Core types for camera control and configuration in the Strand Camera ecosystem
 - vimba (camera/vimba) - low-level Rust bindings for the Allied Vision Vimba SDK
</details>

### `media-utils/` - video and file formats

<details>

 - apriltag-detection-writer (media-utils/apriltag-detection-writer) - write AprilTag detection results to CSV files
 - bg-movie-writer (media-utils/bg-movie-writer) - write video recordings in a background thread
 - burn-timestamps (media-utils/burn-timestamps) - burn timestamps into video frames as rendered text
 - create-timelapse (media-utils/create-timelapse) - create timelapse video from mp4 h264 source without transcoding
 - dump-frame (media-utils/dump-frame) - CLI tool to extract and save individual frames from video files
 - dynlink-cuda (media-utils/nvenc/dynlink-cuda) - dynamic linking for the NVIDIA CUDA runtime library
 - dynlink-nvidia-encode (media-utils/nvenc/dynlink-nvidia-encode) - dynamic linking for the NVIDIA Video Codec SDK (NVENC)
 - ffmpeg-rewriter (media-utils/ffmpeg-rewriter) - save MP4 video by piping frames through ffmpeg and rewriting with metadata upon file close
 - ffmpeg-writer (media-utils/ffmpeg-writer) - write video by piping raw frames through ffmpeg
 - fmf (media-utils/fmf) - read and write .fmf (Fly Movie Format) video files
 - fmf-cli (media-utils/fmf/fmf-cli) - work with .fmf (fly movie format) files
 - font-drawing (media-utils/font-drawing) - draw text onto images
 - frame-source (media-utils/frame-source) - read video frames from MP4, FMF, MKV, and TIFF stack sources
 - gen-nvenc-bindings (media-utils/nvenc/dynlink-nvidia-encode/gen-nvenc-bindings) - code generator for NVIDIA NVENC FFI bindings
 - less-avc-wrapper (media-utils/less-avc-wrapper) - encode video frames to H.264 using the less-avc library
 - mkv-parser-kit (media-utils/mkv-parser-kit) - Library for building Matroska (MKV) file parsers
 - mkv-strand-reader (media-utils/mkv-strand-reader) - read Strand Camera MKV video files with embedded metadata
 - mp4-writer (media-utils/mp4-writer) - write MP4 video files with H.264 encoding
 - nvenc (media-utils/nvenc) - GPU-accelerated H.264 video encoding using NVIDIA NVENC
 - show-timestamps (media-utils/show-timestamps) - display timestamps embedded in Strand Camera video files
 - srt-writer (media-utils/srt-writer) - write SubRip (SRT) subtitle files with timestamp data
 - strand-convert (media-utils/strand-convert) - convert between video formats used in the Strand Camera ecosystem
 - tiff-decoder (media-utils/tiff-decoder) - decode TIFF image stacks from Strand Camera recordings
 - ufmf (media-utils/ufmf) - read and write .ufmf (Uncompressed Fly Movie Format) video files
 - video2rrd (media-utils/video2rrd) - Convert video with Strand Cam timestamps to RRD format for Rerun Viewer
 - y4m-writer (media-utils/y4m-writer) - write raw video frames in YUV4MPEG2 (Y4M) format
</details>

### `geometry/` - camera geometry, calibration, and 3D math

<details>

 - bisection-search (geometry/refraction/bisection-search) - generic bisection search algorithm over ordered fields
 - braid-april-cal (geometry/braid-april-cal) - multi-camera calibration using AprilTag detections and the SQPnP algorithm
 - braid-april-cal-cli (geometry/braid-april-cal/braid-april-cal-cli) - Create a multi-camera calibration using known intrinsics and the SQPnP algorithm
 - braid-april-cal-webapp (geometry/braid-april-cal/braid-april-cal-webapp) - web app for Braid multi-camera calibration using AprilTags
 - braid-apriltag-types (geometry/braid-apriltag-types) - AprilTag detection type definitions for the Braid ecosystem
 - braid-mvg (geometry/braid-mvg) - Braid's camera geometry and multi-view geometry (MVG) types and algorithms.
 - braidz-mcsc (geometry/braidz-mcsc) - multi-camera self-calibration from .braidz tracking data
 - bundle-adj (geometry/bundle-adj) - bundle adjustment for multi-camera calibration
 - camcal (geometry/camcal) - camera calibration utilities using checkerboard patterns
 - find-chessboard (geometry/opencv-calibrate/find-chessboard) - find chessboard corners in an input image
 - flydra-mvg (geometry/flydra-mvg) - Flydra-compatible multi-view geometry with refraction support
 - flytrax-apriltags-calibration (geometry/braid-april-cal/flytrax-apriltags-calibration) - CLI tool for FlyTrax camera calibration using AprilTags
 - freemovr-calibration (geometry/freemovr-calibration) - create calibration for FreeMoVR system
 - freemovr-calibration-cli (geometry/freemovr-calibration/freemovr-calibration-cli) - CLI to create calibration for FreeMoVR system
 - freemovr-calibration-webapp (geometry/freemovr-calibration/freemovr-calibration-webapp) - web app to create calibration for FreeMoVR system
 - mcsc-structs (geometry/mcsc-structs) - data structures and file I/O for the MultiCamSelfCal calibration tool
 - mvg-util (geometry/braid-mvg/mvg-util) - CLI utilities for inspecting and converting camera calibration files
 - ncollide-geom (geometry/freemovr-calibration/ncollide-geom) - create mask from points using ncollide2d
 - opencv-calibrate (geometry/opencv-calibrate) - Rust bindings to OpenCV camera calibration routines
 - parry-geom (geometry/parry-geom) - create collision masks from point sets using the Parry geometry library
 - refraction (geometry/refraction) - compute light refraction at planar interfaces using Snell's law
 - simple-obj-parse (geometry/simple-obj-parse) - parse Wavefront OBJ 3D model files into triangle meshes
 - textured-tri-mesh (geometry/textured-tri-mesh) - textured triangle mesh data type with optional serde support
 - undistort-image (geometry/undistort-image) - undistort images using camera calibration parameters
</details>

### `utils/` - general-purpose utilities

<details>

 - build-util (utils/build-util) - get git hash at build-time
 - csv-eof (utils/csv-eof) - silently handle unexpected EOF when reading truncated CSV files
 - dir2zip (utils/zip-or-dir/dir2zip) - CLI program to convert a directory to a zip file
 - download-verify (utils/download-verify) - download files from URLs and verify their SHA-256 hash
 - env-tracing-logger (utils/env-tracing-logger) - initialize a tracing subscriber configured from environment variables
 - env-tracing-logger-sample (utils/env-tracing-logger/env-tracing-logger-sample) - sample program demonstrating env-tracing-logger usage
 - groupby (utils/groupby) - group sorted iterators by key with lookahead buffering
 - strand-cam-enum-iter (utils/strand-cam-enum-iter) - A utility crate to provide an EnumIter trait for iterating over enums in the Strand Camera ecosystem
 - strand-datetime-conversion (utils/strand-datetime-conversion) - Convert between chrono and f64 time. Used in Strand Camera and Braid.
 - strand-withkey (utils/strand-withkey) - defines the WithKey trait for Strand Camera
 - workspace-docs (utils/workspace-docs) - CLI program to maintain repository overview in workspace README.md
 - write-debian-changelog (utils/write-debian-changelog) - simple and hacky CLI program to print a debian changelog
 - zip-or-dir (utils/zip-or-dir) - read files from either a zip file or a directory
</details>

### `im-proc/` - image processing

<details>

 - ads-apriltag (im-proc/ads-apriltag) - Rust bindings to the AprilTag C library for fiducial marker detection
 - apriltag-track-movie (im-proc/ads-apriltag/apriltag-track-movie) - use ffmpeg to decode input movie and output csv file with april tag detections
 - fastfreeimage (im-proc/fastfreeimage) - fast image processing operations
 - flydra-feature-detector (im-proc/flydra-feature-detector) - detect features in images, maximally backwards compatible with Flydra
 - flydra-feature-detector-types (im-proc/flydra-feature-detector/flydra-feature-detector-types) - Configuration types for Strand Camera, Braid and Flydra feature detection.
 - flydra-pt-detect-cfg (im-proc/flydra-feature-detector/flydra-pt-detect-cfg) - Default values for the flydra-feature-detector-types crate
 - imops (im-proc/imops) - image processing operations, accelerated using SIMD
 - strand-dynamic-frame (im-proc/strand-dynamic-frame) - images from machine vision cameras used in Strand Camera
</details>

### `web/` - web utilities

<details>

 - ads-webasm (web/ads-webasm) - yew components used in Strand Camera and Braid
 - ads-webasm-example (web/ads-webasm/example) - example usage of yew components used in Strand Camera and Braid
 - braid-http-session (web/braid-http-session) - HTTP session for Braid
 - event-stream-types (web/event-stream-types) - types for http event streams
 - strand-bui-backend-session (web/strand-bui-backend-session) - Backend session management for the BUI (Browser User Interface) used by Strand Camera and Braid
 - strand-bui-backend-session-types (web/strand-bui-backend-session/types) - Types for Strand Camera BUI (Browser User Interface) backend session management
 - strand-cam-bui-types (web/strand-cam-bui-types) - Type definitions for the Strand Camera Browser User Interface (BUI) system.
 - strand-http-video-streaming (web/strand-http-video-streaming) - stream video over HTTP
 - strand-http-video-streaming-types (web/strand-http-video-streaming/strand-http-video-streaming-types) - Type definitions for HTTP video streaming functionality in the Strand Camera ecosystem.
</details>

### `led-box/` - LED box and other hardware

<details>

 - led-box (led-box/led-box) - CLI program to interact with LED box hardware directly
 - led-box-standalone (led-box/led-box-standalone) - standalone GUI application for controlling the Strand Camera LED box
 - strand-led-box-comms (led-box/strand-led-box-comms) - Communication protocol types for the Strand Camera LED Box device.
</details>

<!-- workspace-docs end -->
