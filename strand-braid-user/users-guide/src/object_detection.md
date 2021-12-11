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
[ImPtDetectCfg](https://strawlab.org/strand-braid-api-docs/latest/image_tracker_types/struct.ImPtDetectCfg.html)
section of the API.

A more technical account of this procedure can be found in [Straw et al. (2011)](http://dx.doi.org/10.1098/rsif.2010.0230).

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
[TrackingParams](https://strawlab.org/strand-braid-api-docs/latest/flydra_types/struct.TrackingParams.html)
section of the API.

<!--
### Optimization

For the 3D parameters, this is more difficult. I think I have some emails from the past year or two with Floris van Breugel where I discussed this. Let me see if I can find those.
-->
