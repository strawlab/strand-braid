# Calibration

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
distortion model.

## Step 1: setup cameras (zoom, focus, aperture, gain) and lights

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

## Step 2: run "Checkerboard Calibration" to get the camera intrinsic parameters

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

Repeat this for all cameras before proceeding to the next step.

## Step 3: collect calibration data and run MultiCamSelfCal (MCSC)

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

### Convert `.braidz` file to flydra mainbrain `.h5` file

> Note: from here in the document, there are many commands used from
> [Flydra](https://github.com/strawlab/flydra). While Braid itself runs without
> needing Flydra in any way, performing calibration and other data analysis
> steps currently requires the use of Flydra. Specifically, the
> `flydra_analysis` package is required. Please see
> [here](https://github.com/strawlab/flydra#installation) for instructions about
> Flydra installation.

By converting from `.braidz` to a Flydra mainbrain `.h5` file, you can use the
wide range of analysis programs from [flydra](https://github.com/strawlab/flydra).

For example, let's say you have the file `20190924_161153.braidz` saved by the
Braid program. We will use the script `convert_kalmanized_csv_to_flydra_h5.py`
to do this conversion:

    python ~/src/strand-braid/strand-braid-user/scripts/convert_kalmanized_csv_to_flydra_h5.py 20190924_161153.braidz

Upon success, there will be a new file saved with the suffix `.h5`. In this
case, it will be named `20190924_161153.braidz.h5`.

We can do the above but making use of bash variables to save typing later `BRAIDZ_FILE`.

    BRAIDZ_FILE=20190924_161153.braidz
    DATAFILE="$BRAIDZ_FILE.h5"
    python ~/src/strand-braid/strand-braid-user/scripts/convert_kalmanized_csv_to_flydra_h5.py $BRAIDZ_FILE

Note that this conversion requires the program `compute-flydra1-compat` (from
the `braid-offline` package) to be on your path if you are converting 3D
trajectories.

### Run MultiCamSelfCal on data collected with Braid

You can collect data and calibrate similar to [the description for Flydra](https://github.com/strawlab/flydra/blob/c9f20d5f8f4feb7e1fe008cf0ee67fbbc70b1ba0/docs/flydra-sphinx-docs/calibrating.md)

If your mainbrain `.h5` file is in the location `$DATAFILE`, you can run the [MultiCamSelfCal](https://github.com/strawlab/MultiCamSelfCal)
program on this data to generate a multiple camera calibration.

    flydra_analysis_generate_recalibration --2d-data $DATAFILE --disable-kalman-objs $DATAFILE --undistort-intrinsics-yaml=$HOME/.config/strand-cam/camera_info  --run-mcsc --use-nth-observation=4

This will print various pieces of imformation to the console when it runs. First it will print something like this:

```
851 points
by camera id:
 Basler_40022057: 802
 Basler_40025037: 816
 Basler_40025042: 657
 Basler_40025383: 846
by n points:
 3: 283
 4: 568
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

```
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

finished: result in  /home/strawlab/20190924_161153.braidz.h5.recal/result
```

Important things to watch for:

- There should only be one iteration (numbered `0`).

- The mean reprojection error should be low. Certainly less than
  1 pixel and ideally less than 0.5 pixels as shown here.

- The reprojection error should be low across all cameras.

The above example calibration is a good one.

### Convert your new calibration to an XML file which can be used by Braid

Convert this to XML:

    flydra_analysis_calibration_to_xml ${DATAFILE}.recal/result > new-calibration-name.xml

You may now use this new calibration, saved as an XML file, as the calibration
for Braid. Specify the filename of your new XML file as `cal_fname` in the
`[mainbrain]` section of your Braid configuration `.toml` file.

### With the new calibration, perform offline tracking the data used to calibrate.

Using this calibration, you can perform 3D tracking of the data with:

    flydra_kalmanize ${DATAFILE} -r ${DATAFILE}.recal/result

You can view these results with:

    DATAFILE_RETRACKED=`python -c "print('${DATAFILE}'[:-3])"`.kalmanized.h5
    flydra_analysis_plot_timeseries_2d_3d ${DATAFILE} -k ${DATAFILE_RETRACKED} --disable-kalman-smoothing

Now you have a working calibration, which is NOT aligned or scaled to the
flycube coordinate system, but is able to track. Scaling can be quite important
for good tracking.

### Align your new calibration

Next, using this new calibration, collect a dataset which outlines the geometry
of your arena. We will use this to align and scale the unaligned calibration to
an aligned calibration. Easiest is to acquire this dataset directly by running
Braid with the new calibration. Alternatively, one can use a pre-existing 2D dataset
and re-track it with `flydra_kalmanize` as above. We will call this dataset
for alignment `${NEW_TRACKED_DATA}`.

Finally, with this new dataset for alignment, we render the 3D tracks with a 3D
model and adjust the alignement parameters by hand in a GUI. Here we align
our newly tracked data in file `${NEW_TRACKED_DATA}` against the `sample_bowl.xml` file from
[here](https://github.com/strawlab/flydra/blob/master/flydra_analysis/flydra_analysis/a2/sample_bowl.xml).

    flydra_analysis_calibration_align_gui --stim-xml ~/src/flydra/flydra_analysis/flydra_analysis/a2/sample_bowl.xml ${NEW_TRACKED_DATA}
