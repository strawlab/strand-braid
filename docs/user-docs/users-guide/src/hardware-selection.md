# Hardware selection

## PC requirements

* Supports recent Ubuntu Linux Long Term Support (LTS) releases (amd64). This is
  currently our only supported platform.
* Fast CPU. Currently an Intel CPU is recommended due to the use of the Intel
  Integrated Performance Primitives Library.
* Memory usage is not expected to be particularly high, because processing
  occurs in realtime.
* Sufficient and fast interfaces to cameras. If your cameras are USB3 or Gigabit
  ethernet, your computer needs to support enough bandwidth.
* Disk space and speed. For realtime tracking, the tracking data is only modest
  in size and so no particularly high performance requirements exist. With
  hardware-accelerated H.264 encoding (see below), compressed video can be saved
  to MP4 files with only modest CPU and disk requirements. For streaming
  uncompressed, raw video to disk (with MP4 or the FMF format), very fast disks
  and lots of disk space are required.

### Hardware-accelerated video encoding

H.264 video encoding can be offloaded to hardware, freeing the CPU to continue
tracking at full framerate while recording video. Two paths are supported:

**NVIDIA NVENC.** If an NVIDIA GPU with NVENC support is present, Strand Camera
can use it directly via NVIDIA's NVENC library. This requires no additional
software beyond the NVIDIA driver. Please see NVIDIA's site for [supported
hardware](https://developer.nvidia.com/video-encode-and-decode-gpu-support-matrix-new).

**ffmpeg.** Strand Camera can also pipe frames through
[ffmpeg](https://ffmpeg.org/) for encoding. Any hardware encoder that ffmpeg
supports (including NVIDIA NVENC, Intel Quick Sync, and AMD AMF) can be used
this way, provided the appropriate ffmpeg build and drivers are installed.
Software encoding via ffmpeg (e.g. `libx264`) is also possible but will use
substantially more CPU.

## Camera requirements

Basler cameras using the Pylon API and Allied Vision cameras using the Vimba X
API are supported.

### Basler cameras

Due to the use of the Pylon API, any camera which can be used in the Pylon
Viewer can be used in principle. In practice, we regularly test with the
following cameras:

* Basler a2A1920-160umPRO
* Basler a2A1920-160umBAS
* Basler acA1300-200um
* Basler acA640-120gm

### Allied Vision cameras

Due to the use of the Vimba X API, any camera which can be used in the Vimba
Viewer can be used in principle. In practice, we have tested with the following
cameras:

* Allied Vision Alvium 1800 U-240m
