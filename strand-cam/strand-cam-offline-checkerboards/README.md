# strand-cam-offline-checkerboards

This is a simple command-line program which takes a directory of PNG image files
and outputs an intrinsic camera calibration. The output format is identical to
that of [ROS `camera_calibration`
`cameracalibrator.py`](https://wiki.ros.org/camera_calibration/Tutorials/MonocularCalibration).

Here is the usage of the program:

```
Usage: strand-cam-offline-checkerboards <INPUT_DIRNAME> [PATTERN_WIDTH] [PATTERN_HEIGHT]

Arguments:
  <INPUT_DIRNAME>   Input directory name (with .png files)
  [PATTERN_WIDTH]   Width of checkerboard pattern, in number of corners (e.g. 8x8 checks would be 7x7 corners) [default: 7]
  [PATTERN_HEIGHT]  Height of checkerboard pattern, in number of corners (e.g. 8x8 checks would be 7x7 corners) [default: 5]

Options:
  -h, --help  Print help
```

For example, running `strand-cam-offline-checkerboards
checkerboard_debug_20240222_164128 18 8` where
`checkerboard_debug_20240222_164128` is the sample data from
[here](https://strawlab-cdn.com/assets/checkerboard_debug_20240222_164128.zip)
should result in command-line output like the following. Importantly, the file
`checkerboard_debug_20240222_164128.yaml` is created with the calibration
results.

```
[2024-07-23T10:21:45Z INFO  strand_cam_offline_checkerboards] Attempting to find 18x8 chessboard.
[2024-07-23T10:21:45Z INFO  strand_cam_offline_checkerboards] checkerboard_debug_20240222_164128/input_18_8_20240222_164140.png
[2024-07-23T10:21:45Z INFO  strand_cam_offline_checkerboards]     Some(144) corners.
[2024-07-23T10:21:45Z INFO  strand_cam_offline_checkerboards] checkerboard_debug_20240222_164128/input_18_8_20240222_164153.png
[2024-07-23T10:21:45Z INFO  strand_cam_offline_checkerboards]     Some(144) corners.
[2024-07-23T10:21:45Z INFO  strand_cam_offline_checkerboards] checkerboard_debug_20240222_164128/input_18_8_20240222_164205.png
[2024-07-23T10:21:45Z INFO  strand_cam_offline_checkerboards]     Some(144) corners.
[2024-07-23T10:21:45Z INFO  strand_cam_offline_checkerboards] checkerboard_debug_20240222_164128/input_18_8_20240222_164218.png
[2024-07-23T10:21:46Z INFO  strand_cam_offline_checkerboards]     None corners.
[2024-07-23T10:21:46Z INFO  strand_cam_offline_checkerboards] checkerboard_debug_20240222_164128/input_18_8_20240222_164231.png
[2024-07-23T10:21:46Z INFO  strand_cam_offline_checkerboards]     Some(144) corners.
[2024-07-23T10:21:46Z INFO  strand_cam_offline_checkerboards] checkerboard_debug_20240222_164128/input_18_8_20240222_164244.png
[2024-07-23T10:21:46Z INFO  strand_cam_offline_checkerboards]     Some(144) corners.
[2024-07-23T10:21:46Z INFO  strand_cam_offline_checkerboards] checkerboard_debug_20240222_164128/input_18_8_20240222_164257.png
[2024-07-23T10:21:46Z INFO  strand_cam_offline_checkerboards]     Some(144) corners.
[2024-07-23T10:21:46Z INFO  strand_cam_offline_checkerboards] checkerboard_debug_20240222_164128/input_18_8_20240222_164309.png
[2024-07-23T10:21:46Z INFO  strand_cam_offline_checkerboards]     Some(144) corners.
[2024-07-23T10:21:46Z INFO  strand_cam_offline_checkerboards] checkerboard_debug_20240222_164128/input_18_8_20240222_164322.png
[2024-07-23T10:21:46Z INFO  strand_cam_offline_checkerboards]     Some(144) corners.
[2024-07-23T10:21:46Z INFO  strand_cam_offline_checkerboards] checkerboard_debug_20240222_164128/input_18_8_20240222_164334.png
[2024-07-23T10:21:46Z INFO  strand_cam_offline_checkerboards]     Some(144) corners.
[2024-07-23T10:21:46Z INFO  strand_cam_offline_checkerboards] checkerboard_debug_20240222_164128/input_18_8_20240222_164347.png
[2024-07-23T10:21:48Z INFO  strand_cam_offline_checkerboards]     None corners.
[2024-07-23T10:21:48Z INFO  strand_cam_offline_checkerboards] checkerboard_debug_20240222_164128/input_18_8_20240222_164407.png
[2024-07-23T10:21:48Z INFO  strand_cam_offline_checkerboards]     Some(144) corners.
[2024-07-23T10:21:48Z INFO  strand_cam_offline_checkerboards] checkerboard_debug_20240222_164128/input_18_8_20240222_164419.png
[2024-07-23T10:21:48Z INFO  strand_cam_offline_checkerboards]     Some(144) corners.
[2024-07-23T10:21:48Z INFO  strand_cam_offline_checkerboards] checkerboard_debug_20240222_164128/input_18_8_20240222_164432.png
[2024-07-23T10:21:51Z INFO  strand_cam_offline_checkerboards]     None corners.
[2024-07-23T10:21:51Z INFO  strand_cam_offline_checkerboards] Mean reprojection error: 0.4362869016964986
[2024-07-23T10:21:51Z INFO  strand_cam_offline_checkerboards] got calibrated intrinsics: RosOpenCvIntrinsics { is_opencv_compatible: true, p: [[1188.8822710588358, 0.0, 0.0], [0.0, 1188.7505371293905, 0.0], [939.1436018463454, 583.0247938879503, 1.0], [0.0, 0.0, 0.0]], k: [[1188.8822710588358, 0.0, 0.0], [0.0, 1188.7505371293905, 0.0], [939.1436018463454, 583.0247938879503, 1.0]], distortion: Distortion([[-0.23420306276397834, 0.07549873880682875, -7.980337104055248e-6, 6.390067664785299e-5, 0.0]]), rect: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], cache: Cache { pnorm: [[1188.8822710588358, 0.0, 0.0], [0.0, 1188.7505371293905, 0.0], [939.1436018463454, 583.0247938879503, 1.0], [0.0, 0.0, 0.0]], rect_t: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], rti: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] } }
[2024-07-23T10:21:51Z INFO  strand_cam_offline_checkerboards] Saved camera calibration to file: checkerboard_debug_20240222_164128.yaml
```

The file `checkerboard_debug_20240222_164128.yaml` should have contents like this:

```
# Saved by strand-cam-offline-checkerboards at 2024-07-23 13:51:24.460886 +02:00
# Mean reprojection distance: 0.44
image_width: 1920
image_height: 1200
camera_name: checkerboard_debug_20240222_164128
camera_matrix:
  rows: 3
  cols: 3
  data:
  - 1188.8822710588358
  - 0.0
  - 939.1436018463454
  - 0.0
  - 1188.7505371293905
  - 583.0247938879503
  - 0.0
  - 0.0
  - 1.0
distortion_model: plumb_bob
distortion_coefficients:
  rows: 1
  cols: 5
  data:
  - -0.23420306276397834
  - 0.07549873880682875
  - -7.980337104055248e-6
  - 0.00006390067664785299
  - 0.0
rectification_matrix:
  rows: 3
  cols: 3
  data:
  - 1.0
  - 0.0
  - 0.0
  - 0.0
  - 1.0
  - 0.0
  - 0.0
  - 0.0
  - 1.0
projection_matrix:
  rows: 3
  cols: 4
  data:
  - 1188.8822710588358
  - 0.0
  - 939.1436018463454
  - 0.0
  - 0.0
  - 1188.7505371293905
  - 583.0247938879503
  - 0.0
  - 0.0
  - 0.0
  - 1.0
  - 0.0
```