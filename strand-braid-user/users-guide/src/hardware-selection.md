# Hardware selection

## PC requirements

* Supports Ubuntu 20.04 amd64 operating system. This is currently our only
  supported platform.
* Fast CPU. Currently an Intel CPU is recommended due to the use of the Intel
  Integrated Performance Primitives Library.
* Memory usage is not expected to be particularly high, because processing
  occurs in realtime.
* Sufficient and fast interfaces to cameras. If your cameras are USB3 or Gigabit
  ethernet, your computer needs to support enough bandwidth.
* Disk space and speed. For realtime tracking, the tracking data is only modest
  in size and so no particularly high performance requirements exist. With
  Nvidia hardware that supports NVENC hardware-accelerated encoding (see below),
  compressed, H.264 encoded video can be saved to MP4 files with also only
  modest CPU and disk requirements are present. For streaming uncompressed, raw
  video to disk (with MP4 or the FMF format), very fast disks and lots of disk
  space are required.

### Hardware-accelerated video encoding using nvidia video cards

NVIDIA video encoding hardware is optionally used to encode H.264 videos in the
MP4 format. This is very nice because it takes almost no CPU and full framerate
videos can be recorded from your cameras during live tracking with little or no
loss of performance. This depends on NVIDIA's library called NVENC. Please see
NVIDIA's site for [supported
hardware](https://developer.nvidia.com/video-encode-and-decode-gpu-support-matrix-new).
Note in particular the limit of three encode sessions with the consumer
(GeForce) hardware that does not exist on many of their professional (Quadro)
cards.

## Camera requirements

Currently, Basler cameras using the Pylon API are the only supported cameras. We
plan to support cameras from Allied Vision using the Vimba API in late 2021 or 2022.

### Basler cameras

Due to the use of the Pylon API, any camera which can be used in the Pylon
Viewer can be used in principle. In practice, we regularly test with the
following cameras:

* Basler a2A1920-160umPRO
* Basler a2A1920-160umBAS
* Basler acA1300-200um
* Basler acA640-120gm

### Allied Vision cameras (Planned for late 2021 or 2022)

Due to the use of the Vimba API, any camera which can be used in the Vimba
Viewer can be used in principle. In practice, we intend to use the following
cameras:

* Allied Vision Alvium 1800 U-240m
