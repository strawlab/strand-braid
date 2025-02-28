# Calibration in Braid

## What is a calibration?

In our usage here, we refer to a calibration as a model of a camera which allows
us to compute the 2D image location of a given 3D point. Braid can use
calibrations of multiple cameras to calculate the 3D position of a given point
when viewed in 2D camera images.

For a given camera, the calibration is divided into two parts. The "extrinsics",
or extrinsic parameters, define the pose of the camera. This is the 3D position
and orientation of the camera. The "intrinsics", or intrinsic parameters, define
the projection of 3D coordinates relative to the camera to an image point in
pixels. In Braid, the camera model is a pinhole model with warping distortion.
The intrinsic parameters include focal length, the pixel coordinates of the
optical axis, and the radial and tangential parameters of a "plumb bob"
distortion model (also called the Brown-Conrady distortion model).

## XML calibration files in Braid

The XML calibration files used in Braid are backwards-compatible with those from
Braid's predecessor, Flydra. A braid XML calibration file contains multiple
individual camera calibrations and potentially and "global" information, such as
whether there is an air-water interface for use in the case when cameras are
looking down into water.

While the format of the XML file is specific to Braid, the actual camera
calibration parameters are conventional and could be obtained via other
workflows than that described here. For example, the "traditional" calibration
method from [flydra](https://github.com/strawlab/flydra) uses the
[MultiCamSelfCal (MCSC) library](https://github.com/strawlab/MultiCamSelfCal).
There is also the simple [Braid April Tag Calibration
Tool](https://strawlab.org/braid-april-cal-webapp/) tool. There is [a tutorial
Jupyter
notebook](https://github.com/strawlab/dlt-april-cal/blob/main/tutorial.ipynb)
for a manual approach involving April Tags.

## Step 0: setup cameras (zoom, focus, aperture, gain) and lights

Setup camera position, zoom, focus (using an object in the tracking volume) and
aperture (slightly stopped down from wide-open). Exposure times and gains are
set in Strand Cam for each camera individually. Note that if you intend to run
at 100 frames per second, exposure times must be less than 10 milliseconds.
These settings (exposure time and gain) are (unfortunately) currently not saved
in any file, and can be set only in the camera settings GUI (in the browser).
The camera keeps these values persistently when it is on, but if it has been
power cycled, these values will be reset.

Try to obtain a luminance distribution which extends across the entire dynamic
range of your sensor (from intensity values 0 to 255) with very little clipping.

## Step 1: run "Checkerboard Calibration" to get the camera intrinsic parameters for each camera

(There is a script to draw checkerboards as SVG files:
[`draw_checkerboard_svg.py`](https://github.com/strawlab/strand-braid/blob/main/strand-braid-user/scripts/draw_checkerboard_svg.py).)

In Strand Cam, there is a region called "Checkerboard Calibration" which allows
you to calibrate the camera intrinsic parameters. Show a checkerboard to the
camera. You must enter your the checkerboard parameters into the user interface.
For example, a standard 8x8 checkerboard would have 7x7 corners. Try to show the
checkerboard at different distances and angles. Do not forget to show the
checkerboard corners in the corners of the camera field of view. There is a
field which shows the number of checkerboard collected - this should increase as
the system detects checkerboards. When you have gathered a good set of (say, at
least 10) checkerboards, click the "Perform and Save Calibration" button. The
results of this calibration are saved to the directory
`$HOME/.config/strand-cam/camera_info`.

As an alternative to running this procedure live with Strand Camera, you may
operate on a directory of PNG images and [the `strand-cam-offline-checkerboards`
program](https://github.com/strawlab/strand-braid/tree/main/strand-cam/strand-cam-offline-checkerboards).

Regardless of whether you create the YAML file containing the camera intrinsic
parameters, the first lines of the YAML file will contain a comment like the following showing the mean reprojection distance.

```text
# Mean reprojection distance: 0.49
```

The mean reprojection distance is a measure of how well the checkerboard
calibration procedure functioned and shows the mean distance between the images
of the checkerboard corners in the saved images compared to a (re)projection of
the calibration's model of the checkerboard into a synthetic image, and is
measured in units of pixels. The theoretical optimal distance is zero. Typically
one can expect mean reprojection distances of one or two pixels at most.

Repeat this procedure for all cameras before proceeding to the next step.

## Step 2: place April Tags at known 3D locations in the scene

We need to place fiducial markers with known locations in our scene such that
each camera sees several of them. Three markers visible per camera is a
mathematical bare minimum, but more is better. If multiple cameras can see a
single marker, this can be helpful to ensure the calibration of all cameras is
internally consistent.

Store the 3D location of the center of each tag in a `markers-3d-coords.csv`
file like the following:

```csv
id,x,y,z
10,0.265,0.758,0.112
15,-0.520,0.773,0.770
20,-0.241,0.509,0.060
22,-0.501,1.025,1.388
```

This is a CSV file giving the April Tag ID number and the X, Y and Z coordinates
of each marker. The coordinate system must be right-handed and the units of each
coordinate are meters.

## Step 3: record detections of April Tags from each camera

TODO: write this. Quick hint: use Strand Cam to record an april tag detection
CSV file for each camera.

## Step 4: Estimate extrinsic parameters and save Braid .xml calibration file

TODO: write this. Quick hint: Use `braid-april-cal-cli` script.

### Optional: Calibration with water

As described
[here](braid_3d_tracking.md#tracking-in-water-with-cameras-out-of-water), Braid
can track objects in water. To enable this, place the string
`<water>1.333</water>` in the XML camera calibration file.
