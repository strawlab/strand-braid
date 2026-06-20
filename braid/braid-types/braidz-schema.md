# Braidz schema

This document describes the changes to the braidz on-disk schema.

## Calibration files

Camera calibration may be stored in two formats:

- `calibration.toml` — the native parametric format, in which each camera's
  intrinsics are stored exactly once, fully broken down (`fx, fy, cx, cy, skew`
  plus an explicit distortion model). This is the preferred format.
- `calibration.xml` — the legacy flydra format, retained for backward
  compatibility.

When writing, `calibration.xml` is always written, and `calibration.toml` is
additionally written whenever the calibration is representable in the native
format (i.e. every camera has an identity rectification matrix). When reading,
`calibration.toml` is preferred and `calibration.xml` is used as a fallback.

Because `calibration.toml` is purely additive and `calibration.xml` continues to
be written, this change does not require a schema version bump. A future change
that stops writing `calibration.xml` would require a version bump.

## 2

In v2, we introduced the files `reconstruct_latency_usec.hlog` and `reprojection_distance_100x_pixels.hlog` which are in the hdrHistogram format. It is otherwise exactly identical.

## 1

This is the initial release after porting from the .h5 format with the Python API. It is as close as possible to a one-to-one conversion.
