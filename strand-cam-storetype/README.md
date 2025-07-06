# strand-cam-storetype

Type definitions for [Strand Camera's](https://strawlab.org/strand-cam) state
management and browser UI communication.

This crate provides core data structures that represent the complete state of a
Strand Camera instance, including camera settings, recording status, feature detection
configuration, and various processing modes. These types are primarily used for:

- Serializing camera state for the web-based user interface
- Managing recording sessions across different file formats
- Configuring real-time image processing features
- Coordinating LED control and Kalman tracking functionality

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
