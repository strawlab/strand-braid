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
