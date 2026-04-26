# Braid and Strand Camera

This is the User's Guide for [Braid](https://strawlab.org/braid/) and [Strand
Camera](https://strawlab.org/strand-cam/).

## What is Strand Camera?

**Strand Camera** is a single-camera application for live video acquisition,
real-time 2D object tracking, and recording. It runs as a local web server and
is controlled through a browser-based interface. You can also control it
programmatically via Python scripts. Use Strand Camera when you have one camera
and need 2D tracking or video recording, or when setting up and calibrating
cameras that will later be used with Braid.

## What is Braid?

**Braid** is a multi-camera system for real-time 3D tracking. It coordinates
multiple Strand Camera instances — each running on the same machine or on
remote machines — synchronizes their cameras using a hardware trigger, and
fuses the per-camera 2D detections into 3D trajectories. Use Braid when you
need 3D position estimates of one or more moving objects.

## Which should I use?

| Goal | Software |
| :--- | :--- |
| Record video from a single camera | Strand Camera |
| 2D tracking from a single camera | Strand Camera |
| 3D tracking with two or more cameras | Braid |
| Calibrating cameras for use with Braid | Strand Camera (one camera at a time) |

Both Braid and Strand Camera are free and open source. You can find the source
code on [GitHub](https://github.com/strawlab/strand-braid). Issues and feature
requests can be posted on the [GitHub issue
tracker](https://github.com/strawlab/strand-braid/issues).

## Recording video in Strand Camera

Strand Camera can record compressed H.264 video to MP4 files. Recording is
started and stopped from the browser UI or via the [Python scripting
interface](./scripting-with-python.md).

### Post-trigger recording

The **Post Triggering** section in the Strand Camera UI allows you to record
video that includes footage from *before* the recording command was given — a
"time travel" or pre-roll buffer.

To use it:

1. In the **Post Triggering** section, enter a **buffer size** (number of
   frames). Strand Camera will continuously keep this many recent frames in
   memory. A larger buffer means you can go further back in time, but uses more
   RAM.
2. When an event of interest occurs (or has just occurred), click **Post Trigger
   MP4 Recording**. Strand Camera starts an MP4 file that begins with the
   buffered frames, so the recording includes footage from before the trigger.
3. Click the stop recording button to end the recording when done.

The buffer size defaults to 0 (disabled). Set it to a non-zero value before the
experiment begins.

## Getting started

New users should begin with [Hardware selection](./hardware-selection.md) and
then [Installation](./installation.md). To go straight to 3D tracking, proceed
to [Configuring and Launching Braid](./braid_configuration_and_launching.md)
after installation.

### About this book

The source code for this documentation is at
[github.com/strawlab/strand-braid/tree/main/docs/user-docs/users-guide](https://github.com/strawlab/strand-braid/tree/main/docs/user-docs/users-guide).
This book is made with [mdBook](https://rust-lang.github.io/mdBook/).
