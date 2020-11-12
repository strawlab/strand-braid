# machine-vision-formats

Type definitions for working with machine vision cameras. This crate aims
to be a lowest common denominator for working with images from machine vision
cameras from companies such as Basler, FLIR, and AVT.

- `PixelFormat` enum with variants `RGB8`, `MONO8`, `YUV444`, `YUV422`,
  `BayerRGGB8`, and so on.
- The `PixelFormat` enum implements the `FromStr` and `Display` traits, and it
  provides the `bits_per_pixel` method.
- `ImageData` trait definition giving a common interface for image data.
- `Stride` trait definition for strided image data.
- Defines compound traits `OwnedImage` and `OwnedImageStride` which implement
  `Into<Vec<u8>>`, allowing to move image data into a raw `Vec<u8>`.
- Can be compiled without standard library support (`no_std`).

## Potential further improvements

The list of pixel formats variants is currently limited rather limited. Please
submit an issue or, better, pull request for any additions needed.

We could also address the question of how endian-ness and packed-ness are
handled. Currently, these are not specified.

### Test compilation with all feature variants

    cargo build
    cargo +nightly build --no-default-features --features "alloc"
    cargo +nightly build --no-default-features
