# Conformance test data

## Sample images

`left01.jpg` .. `left14.jpg` (no `left10`) are the stereo-calibration sample
images from the OpenCV project, copied verbatim from
`samples/data/left*.jpg` on the OpenCV `4.x` branch
(<https://github.com/opencv/opencv>). They are 640x480 grayscale JPEGs, each
containing a 9x6 inner-corner chessboard. OpenCV is BSD-licensed.

`blank.png` is a small all-blank image used as a negative case (no board).

These files are committed so the conformance suite runs without network access.

## Golden corner coordinates

`golden/<image>.json` holds the sub-pixel corner coordinates produced by the
current `find_chessboard_corners` implementation, in detection order. Each file
records the pattern size and the ordered `corners` array.

The `tests/conformance.rs` harness compares fresh detections against these
goldens (see `TOL_PX` there). To regenerate them after an intentional change to
the detector:

```sh
BLESS_GOLDEN=1 cargo test -p opencv-calibrate --test conformance
```

`golden/calibration.json` pins the end-to-end result of running
`calibrate_camera` on the corners detected across all sample frames (intrinsics,
distortion, mean reprojection error). The `tests/calibration.rs` harness checks
it. Regenerate with:

```sh
BLESS_GOLDEN=1 cargo test -p opencv-calibrate --test calibration
```
