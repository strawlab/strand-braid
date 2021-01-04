# gst-plugin-nvargustime

Gstreamer plugin to extract time from nvidiaargussrc.

See [this](https://devtalk.nvidia.com/default/topic/1056918/jetson-tx2/nvarguscamerasrc-buffer-metadata-is-missing-/post/5392925/#5392925) for more info.

## build and run

Build and run like this:

    cargo build --release
    export GST_PLUGIN_PATH=`pwd`/../target/release

    # to show timestamps on the Jetson Nano:

    gst-launch-1.0 nvarguscamerasrc silent=false ! nvargustime ! fakesink

**As seen above, `nvargustime` must be the pipeline element immediately after `nvarguscamerasrc`.**

Inspect the plugin like this:

    gst-inspect-1.0 nvargustime

## Further reading

Start [here](https://devtalk.nvidia.com/default/topic/1058122/jetson-tx2/argus-timestamp-domain/).

See also [this](https://developer.ridgerun.com/wiki/index.php?title=NVIDIA_Jetson_TX2_-_Video_Input_Timing_Concepts) and [this](https://developer.ridgerun.com/wiki/index.php?title=NVIDIA_Jetson_TX2_-_Video_Input_Timing_Concepts#Timestamping_System_Clock).

## Observations

* The PTS is locked to the timestamp saved by `nvarguscamerasrc` offset by a
fixed amount.
* The timestamp saved by `nvarguscamerasrc` is in units of CLOCK_MONOTONIC

## License

gst-plugin-nvargustime is Copyright (C) 2020 Andrew Straw <strawman@astraw.com>.

Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
http://opensource.org/licenses/MIT>, at your option. This file may not be
copied, modified, or distributed except according to those terms.
