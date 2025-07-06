# strand-dynamic-frame

Images from machine vision cameras used in [Strand Camera](https://strawlab.org/strand-cam).

[![Documentation](https://docs.rs/strand-dynamic-frame/badge.svg)](https://docs.rs/strand-dynamic-frame/)
[![Crates.io](https://img.shields.io/crates/v/strand-dynamic-frame.svg)](https://crates.io/crates/strand-dynamic-frame)

Building on the
[`machine_vision_formats`](https://docs.rs/machine-vision-formats) crate which
provides compile-time pixel formats, this crate provides types for images whose
pixel format is determined at runtime. This allows for flexibility in handling
images data whose pixel format is known only dynamically, such as when reading
an image from disk.

There are two types here:
- `DynamicFrame`: A borrowed view of an image with a dynamic pixel format.
- `DynamicFrameOwned`: An owned version of `DynamicFrame` that contains
  its own buffer.

When compiled with the `convert-image` feature, this crate also provides
conversion methods to convert the dynamic frame into a static pixel format
using the [`convert_image`](https://docs.rs/convert-image) crate.

## Building the documentation

Build and open the docs with:

    RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo +nightly  doc --features convert-image --open

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
<http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
