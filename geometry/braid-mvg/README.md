# braid-mvg

Camera geometry and multi-view geometry (MVG) types and algorithms for the
[Braid](https://strawlab.org/braid) tracking system.

This crate provides camera modeling, geometric transformations, and multi-camera
system support for 3D computer vision applications. It's specifically designed
for use in the Braid multi-camera tracking system but can be used for general
computer vision tasks.

## Features

- Camera modeling with intrinsic and extrinsic parameters based on
  [`cam-geom`](https://crates.io/crates/cam-geom)
- Lens distortion correction using OpenCV-compatible models based on
  [`opencv-ros-camera`](https://crates.io/crates/opencv-ros-camera)
- Multi-camera system management and calibration
- 3D point triangulation from multiple camera views
- Point alignment algorithms (Kabsch-Umeyama, robust Arun)
- Coordinate frame transformations between world, camera, and pixel spaces
- [rerun.io](https://rerun.io) integration for 3D visualization (optional)

## Building the docs

To build and open the docs locally as https://docs.rs/ would do it:

    RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo +nightly doc --all-features --open

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
