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
[`draw_checkerboard_svg.py`](https://github.com/strawlab/strand-braid/blob/main/docs/user-docs/scripts/draw_checkerboard_svg.py).)

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

Regardless of how you created the YAML file containing the camera intrinsic
parameters, the first lines of the YAML file will contain a comment like the
following showing the mean reprojection distance.

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

For each camera, you will record a CSV file of its April Tag detections. Repeat
the following steps for each camera individually before proceeding.

1. Access the camera's Strand Camera instance, either via Braid or by launching
   Strand Camera directly for that camera.

2. In the Strand Cam GUI, deselect "Object detection → enable object detection"
   to turn off object detection.

3. Select "April tag detection → enable April tag detection". If tags are
   visible to the camera, the center of each detected tag will be highlighted
   with a green circle in the live view tab. Verify that the camera detects at
   least three April Tags before proceeding. While three is the mathematical
   minimum, more is better — aim to maximize the number of tags each camera
   can see.

   > **Note:** April Tag detection is computationally demanding. If you
   > encounter frame-drop errors, try running Strand Camera in a separate
   > instance (not via Braid) or reduce the camera frame rate to around 10 FPS
   > for this step. See the troubleshooting section for more details.

   > **Note:** Tags viewed at shallow angles, near the distorted periphery of
   > the field of view, or appearing small in the image may not be detected on
   > every frame, causing the green detection circle to flicker. Such
   > intermittent detections are still suitable for calibration.

4. In the April tag detection tab, click the circular record icon to start
   recording. This saves a gzip-compressed CSV (`.csv.gz`) file in the directory
   from which Braid was launched.

5. Allow recording to run for approximately ten seconds. This ensures that any
   intermittent detections are captured.

6. Click the square stop-recording button (which replaced the record button) to
   end recording.

After repeating steps 1–6 for every camera, move all of the `.csv.gz` files into
a single directory. The path to this directory is used in the next step as the
`--apriltags-2d-detections-dir` argument to `braid-april-cal-cli`.

## Step 4: Estimate extrinsic parameters and save Braid .xml calibration file

### 4a: Run `braid-april-cal-cli`

Run the following command to estimate the extrinsic camera parameters and
produce a Braid XML calibration file:

```sh
braid-april-cal-cli \
  --apriltags-3d-fiducial-coords <3d-coords-file> \
  --apriltags-2d-detections-dir <2d-dir> \
  --intrinsics-yaml-dir <intrinsics> \
  --bundle-adjustment \
  --bundle-adjustment-world-points-remain-fixed \
  --output-xml <output.xml>
```

where:

- `<3d-coords-file>` is the path to the `markers-3d-coords.csv` file from Step 2.
- `<2d-dir>` is the path to the folder of per-camera detection CSV files from
  Step 3.
- `<intrinsics>` is the path to the folder containing the intrinsic calibration
  YAML files for each camera from Step 1, normally
  `~/.config/strand-cam/camera_info`.
- `<output.xml>` is the path where the calibration will be saved; this path
  must end in `.xml`.

To also save a Rerun visualization file for use in the next step, add
`--rerun-save <rerun_save.rrd>` where `<rerun_save.rrd>` is the output path
(must end in `.rrd`). This is optional but recommended — it enables the
validation step 4c.

The `--bundle-adjustment` flag enables an optimization of all calibration
parameters. By default `braid-april-cal-cli` does not alter camera intrinsic
parameters, and the additional flag
`--bundle-adjustment-world-points-remain-fixed` prevents bundle adjustment from
moving the April Tag 3D coordinates. Bundle adjustment is therefore restricted
to the extrinsic (pose) parameters of each camera.

> **Note:** The bundle adjustment options can be altered or removed. Run
> `braid-april-cal-cli --help` for a full list of options. We recommend using
> the restrictions shown above, at least initially.

### 4b: Validate the calibration summary

The tool prints a calibration summary to the terminal; the same summary also
appears in the first lines of `<output.xml>`. The summary has two main
sections: *Results from SQPnP algorithm using prior intrinsics* and *Results
after refinement with bundle adjustment model*. Inspect the latter section and
verify the following:

1. **3d distance between original and updated point locations** — every tag ID
   should show a value of `0.0000`. Non-zero values indicate the
   `--bundle-adjustment-world-points-remain-fixed` constraint was not respected.

2. **3d distance between original and updated camera center locations** —
   adjustments should be small, ideally below 0.05 m.

3. **Camera parameters** — for each camera the transverse (`t_`) and rotation
   (`r_`) values in x, y, and z are listed. Verify that these correspond with
   the known positions and orientations of the cameras in your setup. (The
   intrinsic parameters `fx`, `fy`, `cx`, `cy`, `k1`, `k2`, `k3`, `p1`, `p2`
   are also listed but were not adjusted and can be ignored.)

4. **reprojection distance** — a table reports the reprojection distance (in
   pixels) for each April Tag visible to each camera. The rightmost column
   gives the mean per tag across all cameras that see it; the bottom row gives
   the mean per camera across all tags it sees; and the bottom-right cell gives
   the overall mean across the whole system. Verify that the overall mean is
   below 10 pixels, and ideally below 5 pixels.

If any of the above criteria are not met, you must generate a new calibration.
Use the reprojection distance table to identify the problematic tags or cameras:

- A tag with high error: remeasure its position or move it so that more cameras
  see it, then update `<3d-coords-file>` and repeat from Step 3.
- A camera with high error: adjust its position, viewing angle, or focus, or
  repeat its intrinsic calibration (Step 1), then repeat from Step 3.

> **Note:** Acceptable reprojection values depend on the bundle adjustment
> settings used. The thresholds above apply to the command as written in
> Step 4a.

### 4c: Visualize the calibration in Rerun

```sh
rerun <rerun_save.rrd>
```

Rerun displays a 3D reconstruction of the calibration: numbered tags mark the
centre of each April Tag and pyramid shapes show the camera positions. Each
camera's field of view is also shown, with detected April Tags labelled. Using
the time-series slider at the bottom you can step from the beginning to the
end of bundle adjustment. Verify that the arrangement of cameras and tags
matches your physical setup.

### 4d: Configure Braid to use the calibration

Edit the Braid TOML config file (created when you first launched Braid):

1. Find the line starting with `# cal_fname` (it is commented out by default).
2. Delete the leading `#` to uncomment it.
3. Replace the placeholder path with the path to the file produced in Step 4a:

   ```toml
   cal_fname = "<output.xml>"
   ```

Braid will load this calibration the next time it is launched.

### Optional: Calibration with water

As described
[here](braid_3d_tracking.md#tracking-in-water-with-cameras-out-of-water), Braid
can track objects in water. To enable this, place the string
`<water>1.333</water>` in the XML camera calibration file.
