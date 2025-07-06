# strand-led-box-comms

Communication protocol types for the [Strand
Camera](https://strawlab.org/strand-cam) LED Box device.

This crate provides the data structures and constants for communicating with the
Strand Camera LED Box hardware device over serial communication.

## Testing

`std` is required when running the tests, but otherwise the library is `no_std`.

```
cargo test
```

Test that this remains true by building for a target without std:

    cargo build --no-default-features --target thumbv7em-none-eabihf --features print-defmt


## Building the docs

To build and open the docs locally as https://docs.rs/ would do it:

    RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo +nightly doc --open

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
