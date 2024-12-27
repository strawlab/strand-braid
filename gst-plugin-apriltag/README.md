# gst-plugin-apriltag

## Prerequisites:

This crate requires gstreamer (with the video plugin) libraries.

On Debian/Ubuntu linux, you can install the prerequisites to build like this:

    sudo apt-get install libgstreamer-plugins-base1.0-dev

On Debian/Ubuntu linux, you can install `gst-inspect-1.0` like this:

    sudo apt-get install gstreamer1.0-tools

## Build and run

Build and run like this:

    cargo build --release
    export GST_PLUGIN_PATH=`pwd`/../target/release

    # to detect 36h11 tags (the default):

    gst-launch-1.0 filesrc location=movie-36h11.m4v ! decodebin ! videoconvert ! apriltagdetector ! filesink location=movie-36h11.csv

    # or use the 'family' property to use 'standard_41h12' tags:

    gst-launch-1.0 filesrc location=movie-standard41h12.m4v ! decodebin ! videoconvert ! apriltagdetector family=standard-41h12 ! filesink location=movie-standard41h12.csv

    # to record live on the Jetson Nano:

    gst-launch-1.0 nvarguscamerasrc ! capsfilter caps='video/x-raw(memory:NVMM),width=3820,height=2464,framerate=21/1,format=NV12' ! nvvidconv flip-method=2 ! apriltagdetector ! filesink location=april-out.csv

    # To show webcam live (with v4l2src) on-screen and also save april tag output to file. Note that this does not buffer the writes to disk
    # so they can be seen with "tail -f movie-standard41h12.csv". To buffer, change buffer-mode to "full" or remove the buffer-mode property
    # to accept the default):

    gst-launch-1.0 -v v4l2src ! tee name=t ! queue ! xvimagesink t. ! queue ! videoconvert ! apriltagdetector family=standard-41h12 ! filesink buffer-mode=unbuffered location=movie-standard41h12.csv

Inspect the plugin like this:

    gst-inspect-1.0 apriltagdetector

## Debug

You can also use the environment variables `GST_DEBUG=2,apriltagdetector:6` to
perform more debugging.

For example:

    GST_DEBUG=2,apriltagdetector:6 gst-launch-1.0 videotestsrc num-buffers=5 is-live=1 ! apriltagdetector family=16h5 ! filesink location=trash.csv

## License

Like apriltag itself, gst-plugin-apriltag is licensed under the BSD-2-Clause license.

Portions of the code derive from the gst-plugin tutorial (C) 2018 Sebastian
Dröge, licensed under the Apache License, Version 2.0  or the MIT license.
