## Building

This crate builds the single `strand-cam` executable, which supports the
Basler Pylon and Allied Vision Vimba camera backends, plus a consumer webcam
backend intended for development use. The backend is selected at runtime with
the `--camera-backend pylon|vimba|webcam` argument (defaulting to pylon). The
vendor SDKs are loaded dynamically at runtime, so neither is required to
build, and the webcam backend needs no SDK at all.

To check the build

    cargo check --bin strand-cam --features "serve_files checkercal fiducial flydra_feat_detect"

To build (debug)

    cargo build --bin strand-cam --features "serve_files checkercal fiducial flydra_feat_detect"

To build (release)

    cargo build --release --bin strand-cam --features "bundle_files checkercal fiducial flydra_feat_detect"

Note: with the `bundle_files` feature, the browser frontend in `yew_frontend`
is compiled by `trunk` and embedded into the executable automatically by
`build.rs`. With `serve_files`, the frontend is instead served from
`yew_frontend/dist` at runtime (useful for frontend development), so build it
first in that case (`cd yew_frontend && ./build.sh`).

Note, on Windows, due to [limitations on /clr compilation](https://msdn.microsoft.com/en-us/library/ffkc918h.aspx), the [Visual C++  Redistributable](https://support.microsoft.com/en-us/help/2977003/the-latest-supported-visual-c-downloads) may need to be installed to run properly.

## TODO

[ ] In the single-camera case: when the program is running already and a second
    instance is started, somehow detect this and re-open the original browser.

[ ] Save background image and FPS to flydra
[ ] Stop saving 2d CSV/JPG (in favor of flydra2 csv dir)?


--------------

[ ] view bg image

[ ] track fly body angle
[ ] fix d2 > d1 flydra2 error
[ ] show LED on zone in camera image
[ ] save kalman tracking + LED config to disk
[ ] show trail of past tracking data
[ ] log messages should show crate of origin
[ ] add pulsing LEDs in addition to ConstantOn
[ ] debug memory usage in browser
[ ] when saving kalman data file, continue saving if kalman params changed?
