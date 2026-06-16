# Setting and optimizing parameters for 3D Tracking

## Object Detection

The basis of 3D Tracking is a 2D object detection procedure, usually performed
on simultaneously acquired images from at least two cameras.

Object detection is based on background subtraction and feature extraction. In
Braid, these parameters are typically set in the .toml config file specified
when starting the program. When not explicitly specified, default parameters are
used. Within Strand Camera, including when run from within Braid, these
parameters can be set in a running instance. The parameters are specified in a
camera-specific way, meaning that each camera can have its own parameter values.

In Strand Camera, the option `Record CSV file` will record the object detection
results in CSV format with a header including the object detection parameters in
use at the start of the recording.

The details on implementation and parameters can be found in the
[ImPtDetectCfg](https://strawlab.org/strand-braid-api-docs/latest/flydra_feature_detector_types/struct.ImPtDetectCfg.html)
section of the API.

A more technical account of this procedure can be found in [Straw et al. (2011)](http://dx.doi.org/10.1098/rsif.2010.0230).

### How background subtraction works

Object detection operates on luminance (monochrome, 8-bit) images. Images from
color cameras are converted first: RGB pixels are converted to luma using the
standard BT.601 weights (Y ≈ 0.3 R + 0.59 G + 0.11 B), and raw Bayer-format
images are demosaiced to RGB and then converted to luma. Detection is
therefore most sensitive to green contrast for color cameras.

The background model maintains, per pixel, a running mean and a running mean
of squared values (from which a per-pixel standard deviation is derived), both
in 32-bit floating point. When Strand Camera starts, the model is initialized
by averaging the first 20 frames; no features are detected during this brief
startup period. This happens whether or not continuous background updating
(`do_update_background_model`) is enabled. When updating is enabled, the model
is updated every `bg_update_interval` frames by blending in the current frame
with weight `alpha`.

A pixel is detected as part of a feature when its difference from the
background mean (with sign according to `polarity`) exceeds a threshold. With
`use_cmp` enabled, the threshold is per-pixel and adaptive: `n_sigma` times
the running standard deviation of that pixel, but never less than
`diff_threshold`. With `use_cmp` disabled, the fixed `diff_threshold` is used
everywhere.

One subtlety when tuning `diff_threshold` live with `use_cmp` enabled: the
per-pixel thresholds are stored with the `diff_threshold` floor already
applied, and *lowering* `diff_threshold` cannot restore the values underneath
the old floor. The lower floor takes full effect when the per-pixel thresholds
are next recomputed — at the next background model update, or immediately
after pressing one of the background reset buttons described below. (Raising
`diff_threshold` takes effect immediately.)

### Background model controls in the browser UI

The object detection panel in Strand Camera's browser interface has two
buttons affecting the background model:

- **Take Current Image As Background** — discards the current model and
  re-initializes it from the next 20 frames, exactly as at startup. Use this
  after changing the scene or lighting, especially when continuous updating is
  disabled.
- **Set background to mid-gray** — sets the background mean to a uniform value
  of 127 with zero variance.

When running Braid, the Braid browser interface has a "Background Model"
section with buttons that act on all connected cameras at once: **Take New
Background Image**, **Enable Background Updating**, and **Disable Background
Updating**. The per-camera background updating state is shown in the camera
list.

Like everything in the browser interface, these buttons can also be triggered
programmatically, including on all cameras of a Braid setup at once; see the
background reset demos in [Scripting with
Python](scripting-with-python.md#demo-resetting-the-object-detection-background-model-using-python).

<!--
### Optimization

 To debug these values for your setup, I recommend saving data to using flydra and inspecting the 2D points detected. I find the flydra_analysis_plot_timeseries_2d_3d program to be most helpful for this. Flydra was designed to accept quite a few false positives at the 2D stage to avoid having any missed detections, so I would err on the side of accepting too many, rather than too few, 2D features detected. Of course too many 2D detections is also problematic, so this requires some tuning. Hopefully the defaults are a good start for your lighting setup.

There is unfortunately no easy procedure for optimizing parameters. For optimizing 2D feature detection parameters, one should examine the features detected in the 2D view (e.g. with the braidz viewer website or relevant notebooks) and make sure that detections are present at times and locations where they should be and absent from times and locations where they should not be.

-->

## 3D Tracking

3D tracking is based on data association, which links 2D features from
individual cameras to a 3D model, and an Extended Kalman Filter, which updates
the estimated position and velocity of the 3D model from the 2D features.

The implementation details for the 3D tracking procedures can be found in the
[TrackingParams](https://strawlab.org/strand-braid-api-docs/latest/braid_types/struct.TrackingParams.html)
section of the API.

<!--
### Optimization

For the 3D parameters, this is more difficult. I think I have some emails from the past year or two with Floris van Breugel where I discussed this. Let me see if I can find those.

A principled approach would start with ideas such as these:

 - https://www.robots.ox.ac.uk/~ian/Teaching/Estimation/LectureNotes2.pdf
 - https://arxiv.org/pdf/1807.08855.pdf
-->
