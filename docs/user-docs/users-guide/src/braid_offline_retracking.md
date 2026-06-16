# Offline retracking of `.braidz` files

While Braid runs, it produces 3D position estimates (the Kalman estimates) at
low latency. To keep latency low, the online tracker never waits for late or
out-of-order data: any 2D detection that arrives after Braid has already moved on
to a later frame is discarded from the *online* 3D reconstruction. However, every
2D detection is still saved to the `.braidz` file as it arrives.

*Retracking* re-runs the tracker offline over a recorded `.braidz` file, using
**all** of the saved 2D detections for each frame. Because latency is no longer a
concern, the offline reconstruction can incorporate data the online tracker had
to drop, and can also use information from frames acquired *after* a given
instant. See [3D Tracking in Braid](./braid_3d_tracking.md#details-about-how-data-are-processed-online-and-saved-for-later-analysis)
for the underlying rationale.

Retracking is performed by the `braid-offline-retrack` program.

## Retrack with the original parameters and calibration

To recompute the 3D trajectories using the same calibration and tracking
parameters that were used during the live recording:

```sh
braid-offline-retrack --data-src <input.braidz> --output <output.braidz>
```

- `--data-src` / `-d` is the input `.braidz` file to retrack.
- `--output` / `-o` is the output file; its name **must end in `.braidz`**.

`braid-offline-retrack` will **not** overwrite an existing file, so `--output`
must be a new path (and must differ from `--data-src`).

Typical improvements after retracking include:

- 3D estimates added or adjusted for frames where they were missing or noisy
  online.
- Trajectory fragments of the same object that were given different object IDs
  online may be unified into a single object.
- Spurious low-confidence estimates near the end of a track may be removed.
- Object IDs in the output are renumbered starting from 0.

## Retrack with different tracking parameters or a different calibration

You can also retrack with alternative tracking parameters and/or a different
(for example, higher-quality) calibration:

```sh
braid-offline-retrack \
  --data-src <input.braidz> \
  --output <output.braidz> \
  --tracking-params <tracking-params.toml> \
  --new-calibration <new-calibration.xml>
```

- `--tracking-params` is a TOML file of tracking parameters. See
  [Parameters for Object Detection and Tracking](./parameters_for_object_detection_and_tracking.md)
  and
  [`braid_types::TrackingParams`](https://strawlab.org/strand-braid-api-docs/latest/braid_types/struct.TrackingParams.html).
- `--new-calibration` is a calibration file produced as described in
  [Calibration in Braid](./braid_calibration.md).

Both options are independent; you may supply either, both, or neither. Supplying
neither is equivalent to the first command above.

Other options (`--fps`, `--start-frame`, `--stop-frame`) are available; run
`braid-offline-retrack --help` for the full list.

## Visualizing the result

The output `.braidz` file can be inspected with the same tools as any other
`.braidz` file — see [BRAIDZ files and Analysis
Scripts](./braidz-files.md) — including opening it in the
[Braidz viewer](https://braidz.strawlab.org/) or visualizing it in
[Rerun](https://rerun.io/).
