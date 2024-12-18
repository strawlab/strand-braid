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
Braid's predecessor, Flydra. Several of the steps described here also use tools
from Flydra.

## Step 0: setup cameras (zoom, focus, aperture, gain) and lights

Setup camera position, zoom, focus (using an object in the tracking volume) and
aperture (slightly stopped down from wide-open). Exposure times and gains are
set in Strand Cam for each camera individually. Note that if you intend to run
at 100 frames per second, exposure times must be less than 10 milliseconds.
These settings (exposure time and gain) are (unfortunately) currently not saved
in any file, and can be set only in the camera settings GUI (in the browser).
The camera keeps these values persistently when it is on, but if it has been
power cycled, it will reset to new values.

Try to a luminance distribution which extends across the entire dynamic range of
your sensor (from intensity values 0 to 255) with very little clipping.

## Methods for calibration

The "traditional" calibration method, inherited from
[flydra](https://github.com/strawlab/flydra), uses the [MultiCamSelfCal MCSC
library](https://github.com/strawlab/MultiCamSelfCal).

Other methods of calibration are in development. For example, the [Braid April
Tag Calibration Tool](https://strawlab.org/braid-april-cal-webapp/). There is
also [a tutorial Jupyter
notebook](https://github.com/strawlab/dlt-april-cal/blob/main/tutorial.ipynb)
for a manual approach involving April Tags. These April Tag approaches may be
particularly interesting in a setting where Braid is used for tracking in a
Virtual Reality setup where computer displays can be used to show April Tags.

## MCSC Method, Step 1: run "Checkerboard Calibration" to get the camera intrinsic parameters

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

Repeat this for all cameras before proceeding to the next step.

## MCSC Method, Step 2: collect calibration data and run MultiCamSelfCal (MCSC)

To calibrate Braid, we use
[MultiCamSelfCal](https://github.com/strawlab/MultiCamSelfCal). This is a
software package which takes simultaneously captured images of an LED moving
through a volume to create a single calibration for all cameras.

### Acquire a dataset for MultiCamSelfCal

Room lights should be off.

Synchronize your cameras and start saving data and begin to collect data by
moving a LED in the arena (try move the LED in the whole arena volume, also turn
it off and on sometimes to validate synchronization). When you are done, stop
saving.

### Run MultiCamSelfCal on data collected with Braid

Collect data and calibrate similar to [the description for
Flydra](https://github.com/strawlab/flydra/blob/main/docs/flydra-sphinx-docs/calibrating.md)

We will run the `braidz-mcsc` program to export the data for MCSC and then to
run MCSC. Use something like the following, updating for your specific case:

```ignore
braidz-mcsc --input 20241017_164418.braidz --checkerboard-cal-dir ~/.config/strand-cam/camera_info --use-nth-observation 4
```

Here:

- `--input 20241017_164418.braidz` specifieds the name of the file, saved by
  Braid, containing the collected data to be used by MCSC.
- `--checkerboard-cal-dir ~/.config/strand-cam/camera_info` specifies the
  directory containing `.yaml` files saved by
  `strand-cam-offline-checkerboards`.
- `--use-nth-observation 4` indicates that only every 4th frame of data should
  be exported. See below.

This will print various pieces of information to the console when it runs. First it will print something like this:

```ignore
851 points
by camera id:
 Basler_40022057: 802
 Basler_40025037: 816
 Basler_40025042: 657
 Basler_40025383: 846
by n points:
 3: 283
 4: 568
Saved to directory "20241017_164418.braidz.mcsc".
```

This means that 851 frames were acquired in which 3 or more cameras detected exactly one point.
The contribution from each camera is listed. In this example, all cameras had between 657 and
846 points. Then, the number of points detected by exactly 3 cameras is shown (283 such frames)
and exactly 4 cameras (568 frames).

Important things to watch for are listed here. These may be useful, but are rather
rough guidelines to provide some orientation:

- No camera should have very few points, otherwise this camera will not contribute much to
  the overall calibration and will likely have a bad calibration itself.

- The number of points used should be somewhere between 300 and 1000. Fewer than 300 points
  often results in poor quality calibrations. More than 1000 points results in the calibration
  procedure being very slow. The `--use-nth-observation` command line argument can be used
  to change the number of points used.

After running successfully, the console output should look something like this:

```ignore
********** After 0 iteration *******************************************
RANSAC validation step running with tolerance threshold: 10.00 ...
RANSAC: 1 samples, 811 inliers out of 811 points
RANSAC: 2 samples, 811 inliers out of 811 points
RANSAC: 2 samples, 762 inliers out of 762 points
RANSAC: 1 samples, 617 inliers out of 617 points
811 points/frames have survived validations so far
Filling of missing points is running ...
Repr. error in proj. space (no fact./fact.) is ...  0.386139 0.379033
************************************************************
Number of detected outliers:   0
About cameras (Id, 2D reprojection error, #inliers):
CamId    std       mean  #inliers
  1      0.41      0.41    762
  2      0.33      0.33    811
  3      0.49      0.41    617
  4      0.33      0.37    811
***************************************************************
**************************************************************
Refinement by using Bundle Adjustment
Repr. error in proj. space (no fact./fact./BA) is ...  0.388949 0.381359 0.358052
2D reprojection error
All points: mean  0.36 pixels, std is 0.31

Unaligned calibration XML saved to 20241017_164418-unaligned.xml
```

Important things to watch for:

- There should only be one iteration (numbered `0`).

- The mean reprojection error should be low. Certainly less than
  1 pixel and ideally less than 0.5 pixels as shown here.

- The reprojection error should be low across all cameras.

The above example calibration is a good one.

In the example above, the file `20241017_164418-unaligned.xml` was created. You
may now use this new calibration, saved as an XML file, as the calibration for
Braid. Specify the filename of your new XML file as `cal_fname` in the
`[mainbrain]` section of your Braid configuration `.toml` file.

### With the new calibration, perform offline tracking the data used to calibrate.

Now you have a working calibration, which is NOT aligned or scaled to any
coordinate system, but in an undefined coordinate system that the MCSC code
picked. We can use this calibration to do tracking, although in general having
correct scaling is important for good tracking. The reason correct scaling is
important for good quality tracking is because Braid tracks using a dynamic
model of movement in which maneuverability is parameterized and performance is
best when the actual maneuverability matches the expected statistical
paramterization.

Now we will take our existing "unaligned" calibration, and despite the scaling
and alignment issue, track some points so that we have 3D coordinates. We will
use these 3D coordinates to "align" our calibration -- to adjust its scaling,
rotation, and translation to arrive at a final calibration which outputs
coordinates in the desired frame.

(TODO: add more detail here.) Using the unaligned calibration from above,
collect a dataset which contains known 3D reference locations. Using the Braid
browser UI, note the 3D location of each 3D reference location as founded by
Braid using the unaligned calibration. Save the unaligned and reference 3D
locations to `.csv` files with the following schema:

```csv
x,y,z
1,2,3
4,5,6
```

Now run the `align-calibration` script like so:

```ignore
align-calibration --ground-truth-3d reference.csv --unaligned-3d unaligned.csv --unaligned-cal unaligned-calibration.xml --output-aligned-cal aligned-calibration.xml
```

If succesful, this will print various diagnostic information concluding with a
section `Mean distance between ground truth and transformed points`. This can be
interpreted as the quality of the alignment where smaller numbers are better and
zero is perfect. As with all usage of braid, distances are specified in meters.

### Calibration with water

As described
[here](braid_3d_tracking.md#tracking-in-water-with-cameras-out-of-water), Braid
can track objects in water. To enable this, place the string
`<water>1.333</water>` in the XML camera calibration file.
