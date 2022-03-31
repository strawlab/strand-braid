## Building

To build the frontend

    cd yew_frontend
    ./build.sh

To check the build

    cargo check --features "serve_files backend_pyloncxx with_led_box flydratrax flydra2/serve_files"

To build the backend (debug)

    cargo build --features "serve_files backend_pyloncxx with_led_box flydratrax"

To build the backend (release)

    cargo build --release --features "bundle_files backend_pyloncxx"

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
