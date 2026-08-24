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

Every parameter has a default value, so a configuration only needs to list the
parameters it changes. A misspelled parameter name is an error rather than being
silently ignored.

A more technical account of this procedure can be found in [Straw et al. (2011)](http://dx.doi.org/10.1098/rsif.2010.0230).

### Restricting detection to a region of the image

By default, points detected anywhere in the image are used. The `valid_region`
parameter confines detection to part of the image; anything outside is ignored,
as though the image were blank there. This is how you keep a reflection, a
window, or a bit of moving apparatus from being tracked as an object.

The setting is per camera, so each camera has its own region. There are four
forms:

| form | region |
|---|---|
| `Everything` | the whole image (the default) |
| `Circle` | inside one circle |
| `MultipleCircles` | inside *any* of several circles |
| `Polygon` | inside a closed outline of vertices |

All coordinates are in pixels of the full camera image, with x measured to the
right and y downward from the top-left corner.

There is no way to specify the *opposite* of a region — you describe where
detection is allowed, not where it is forbidden — and a polygon cannot have
holes. To allow detection in several disconnected places, use
`MultipleCircles`.

#### Circles

A circle is a center and a radius, in whole pixels:

```toml
[[cameras]]
name = "Basler-40116750"

[cameras.point_detection_config.valid_region.Circle]
center_x = 320
center_y = 256
radius = 200
```

Several circles are written as a list, and detection is allowed inside any of
them:

```toml
[[cameras]]
name = "Basler-40116750"

[[cameras.point_detection_config.valid_region.MultipleCircles]]
center_x = 160
center_y = 256
radius = 120

[[cameras.point_detection_config.valid_region.MultipleCircles]]
center_x = 480
center_y = 256
radius = 120
```

#### Polygons

A polygon is a list of `[x, y]` vertices:

```toml
[[cameras]]
name = "Basler-40116750"

[cameras.point_detection_config.valid_region.Polygon]
points = [
    [100.0, 50.0],
    [600.0, 50.0],
    [600.0, 400.0],
    [350.0, 480.0],
    [100.0, 400.0],
]
```

Since every other object detection parameter has a default, nothing else needs
to be given.

The vertices are read as a ring **in the order you list them**: each connects
to the next, and the last connects back to the first. There is no need to
repeat the first vertex at the end, and either winding direction works.
Vertices are not restricted to whole pixels.

Concave outlines are honored, so a region can exclude part of its own bounding
area — an arena with a bite taken out of it, or a U-shaped walkway:

```toml
[cameras.point_detection_config.valid_region.Polygon]
points = [
    [0.0, 0.0],
    [640.0, 0.0],
    [640.0, 512.0],
    [400.0, 512.0],
    [400.0, 150.0],
    [240.0, 150.0],
    [240.0, 512.0],
    [0.0, 512.0],
]
```

Because vertex order defines the outline, listing the vertices of an otherwise
ordinary convex shape in a scrambled order describes an outline that crosses
itself, which encloses no well-defined region. Rather than tracking nothing,
such a polygon falls back to the convex hull of its vertices. If a region does
not look the way you expect, check that consecutive vertices are neighbors as
you go around the outline. A polygon with fewer than three distinct vertices,
or with all vertices in a line, encloses nothing at all and is reported as an
error.

#### Checking and changing the region

The region in use is drawn on the live image in the Strand Camera browser
interface, so you can confirm it covers what you intended. It can also be
changed there, by editing the YAML in the "Detailed configuration" box of the
object detection panel:

```yaml
valid_region:
  Polygon:
    points:
    - [100.0, 50.0]
    - [600.0, 50.0]
    - [600.0, 400.0]
    - [350.0, 480.0]
    - [100.0, 400.0]
```

A change made there takes effect on the next frame; the background model is
left as it is, so no restart is needed. Changes made in the browser are not
written back to the Braid config file, so put the region in the config file to
make it permanent.

Every recording stores the parameters each camera was using, including its
region, in `feature_detect_settings/<camera>.toml` inside the `.braidz` file.

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
