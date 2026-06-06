# ci2-webcam

A [`ci2`](../ci2) camera backend for consumer webcams (UVC and similar),
intended as a **development convenience** for running Strand Camera without
high-end machine-vision hardware.

Frame capture is provided by the [`nokhwa`](https://crates.io/crates/nokhwa)
crate (`input-native` feature), which uses V4L2 on Linux, AVFoundation on
macOS, and Media Foundation on Windows. The backend is named `webcam` rather
than `nokhwa` so the underlying capture library can be replaced later without
changing the user-facing backend name.

## Usage

The webcam backend is compiled into the `strand-cam` binary alongside the
Pylon and Vimba backends. Pylon remains the default; select the webcam backend
explicitly:

```sh
strand-cam --camera-backend webcam
```

A specific device can be chosen by its human-readable name (as printed at
startup) or its index:

```sh
strand-cam --camera-backend webcam --camera-name "Integrated Camera"
strand-cam --camera-backend webcam --camera-name 0
```

If `--camera-name` is omitted, the first enumerated webcam is used.

## Capabilities

Webcams expose almost none of the controls that machine-vision cameras do.

- **Supported:** device enumeration/selection, frame capture, sensor
  width/height, and pixel format selection between `RGB8` (default) and
  `Mono8`. Each frame (commonly YUYV or MJPEG) is decoded on the host.
- **Unsupported:** hardware triggering, exposure time (µs), gain (dB),
  node-map save/load, frame-rate limiting, and the generic GenICam feature
  accessors. These return `ci2::Error::FeatureNotPresent`. Strand Camera's
  startup path tolerates this error for the values it reads.

## Scripts

This crate has no scripts.
