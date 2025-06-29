//! Type definitions for the Strand Camera Browser User Interface (BUI) system.
//!
//! This crate provides core data structures used in the Strand Camera ecosystem
//! for recording path management and clock synchronization between different
//! timing sources. These types are shared between the camera backend and the
//! web-based user interface.
//!
//! ## Core Types
//!
//! - [`RecordingPath`]: Manages file paths with timestamps and size tracking
//! - [`ClockModel`]: Linear clock synchronization between different time sources
//!
//! ## Features
//!
//! - Serialization support via serde for network communication
//! - UTC timestamp tracking for recording sessions
//! - Clock drift compensation for multi-camera synchronization

// Copyright 2020-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![warn(missing_docs)]

use serde::{Deserialize, Serialize};

/// Path to a recording file with associated metadata and timing information.
///
/// This structure tracks recording file paths along with when recording started
/// and optionally the current file size. It's used throughout the Strand Camera
/// system to manage active recordings for various file formats (MP4, FMF, UFMF, CSV).
///
/// The start time is automatically set to the current UTC time when created,
/// providing a timestamp for when the recording session began.
///
/// # Examples
///
/// ```rust
/// use strand_cam_bui_types::RecordingPath;
///
/// // Create a new recording path
/// let recording = RecordingPath::new("/path/to/video.mp4".to_string());
/// println!("Recording started at: {}", recording.start_time());
/// ```
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct RecordingPath {
    /// The filesystem path to the recording file
    path: String,
    /// UTC timestamp when recording started
    start_time: chrono::DateTime<chrono::Utc>,
    /// Current size of the recording file in bytes (if known)
    current_size_bytes: Option<usize>,
}

impl RecordingPath {
    /// Creates a new recording path with the current UTC time as the start time.
    ///
    /// # Arguments
    ///
    /// * `path` - The filesystem path to the recording file
    ///
    /// # Returns
    ///
    /// A new [`RecordingPath`] instance with the current UTC time as the start time
    /// and no file size information.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use strand_cam_bui_types::RecordingPath;
    ///
    /// let recording = RecordingPath::new("/path/to/video.mp4".to_string());
    /// ```
    pub fn new(path: String) -> Self {
        let start_time = chrono::Utc::now();
        RecordingPath::from_path_and_time(path, start_time)
    }

    /// Creates a new recording path with a specific start time.
    ///
    /// This method is useful when you need to recreate a recording path from
    /// stored data or when you want to specify an exact start time.
    ///
    /// # Arguments
    ///
    /// * `path` - The filesystem path to the recording file
    /// * `start_time` - The UTC timestamp when recording started
    ///
    /// # Returns
    ///
    /// A new [`RecordingPath`] instance with the specified path and start time.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use strand_cam_bui_types::RecordingPath;
    /// use chrono::{DateTime, Utc};
    ///
    /// let start_time = Utc::now();
    /// let recording = RecordingPath::from_path_and_time(
    ///     "/path/to/video.mp4".to_string(),
    ///     start_time
    /// );
    /// ```
    pub fn from_path_and_time(path: String, start_time: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            path,
            start_time,
            current_size_bytes: None,
        }
    }

    /// Returns the filesystem path to the recording file.
    ///
    /// # Returns
    ///
    /// A clone of the recording file path.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use strand_cam_bui_types::RecordingPath;
    ///
    /// let recording = RecordingPath::new("/path/to/video.mp4".to_string());
    /// assert_eq!(recording.path(), "/path/to/video.mp4");
    /// ```
    pub fn path(&self) -> String {
        self.path.clone()
    }

    /// Returns the UTC timestamp when recording started.
    ///
    /// # Returns
    ///
    /// The UTC timestamp when this recording session began.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use strand_cam_bui_types::RecordingPath;
    ///
    /// let recording = RecordingPath::new("/path/to/video.mp4".to_string());
    /// println!("Recording started at: {}", recording.start_time());
    /// ```
    pub fn start_time(&self) -> chrono::DateTime<chrono::Utc> {
        self.start_time
    }

    /// Returns the current size of the recording file in bytes, if known.
    ///
    /// This value is optional and may not be available for all recording types
    /// or during certain phases of recording.
    ///
    /// # Returns
    ///
    /// The current file size in bytes, or `None` if not available.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use strand_cam_bui_types::RecordingPath;
    ///
    /// let recording = RecordingPath::new("/path/to/video.mp4".to_string());
    /// match recording.current_size_bytes() {
    ///     Some(size) => println!("Recording size: {} bytes", size),
    ///     None => println!("Recording size unknown"),
    /// }
    /// ```
    pub fn current_size_bytes(&self) -> Option<usize> {
        self.current_size_bytes
    }

    /// Updates the current size of the recording file.
    ///
    /// This method is typically called by the recording system to update
    /// the file size as data is written to disk.
    ///
    /// # Arguments
    ///
    /// * `size` - The new file size in bytes, or `None` to clear the size information
    ///
    /// # Examples
    ///
    /// ```rust
    /// use strand_cam_bui_types::RecordingPath;
    ///
    /// let mut recording = RecordingPath::new("/path/to/video.mp4".to_string());
    /// recording.set_current_size_bytes(Some(1024));
    /// assert_eq!(recording.current_size_bytes(), Some(1024));
    /// ```
    pub fn set_current_size_bytes(&mut self, size: Option<usize>) {
        self.current_size_bytes = size;
    }
}

/// Linear clock synchronization model for multi-camera systems.
///
/// This structure implements a linear transformation to synchronize timestamps
/// between different clock sources (e.g., camera hardware clocks vs. host system clock).
/// The transformation follows the equation: `host_time = gain * device_time + offset`.
///
/// The clock model is essential for multi-camera synchronization in the Strand Camera
/// system, allowing timestamps from different sources to be aligned to a common
/// time reference.
///
/// # Mathematical Model
///
/// The linear relationship is: **t_host = gain × t_device + offset**
///
/// - `gain`: Clock rate ratio (typically close to 1.0)
/// - `offset`: Time offset between clock sources
/// - `residuals`: Sum of squared residuals from the linear fit
/// - `n_measurements`: Number of data points used to compute the model
///
/// # Examples
///
/// ```rust
/// use strand_cam_bui_types::ClockModel;
///
/// // Create a clock model with typical values
/// let clock_model = ClockModel {
///     gain: 1.000001,           // Slightly faster device clock
///     offset: -1234567.89,      // Device clock started earlier
///     residuals: 0.001,         // Good fit quality
///     n_measurements: 100,      // Based on 100 sync points
/// };
///
/// // Convert device timestamp to host timestamp
/// let device_time = 1000.0;
/// let host_time = clock_model.gain * device_time + clock_model.offset;
/// ```
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct ClockModel {
    /// Clock rate ratio between device and host clocks.
    ///
    /// This represents how fast the device clock runs relative to the host clock.
    /// A value of 1.0 means identical rates, > 1.0 means the device clock runs faster,
    /// and < 1.0 means it runs slower.
    pub gain: f64,

    /// Time offset between device and host clocks.
    ///
    /// This is the constant offset needed to align the two time sources.
    /// The offset accounts for differences in when the clocks were started
    /// and any systematic time differences.
    pub offset: f64,

    /// Sum of squared residuals from the linear regression fit.
    ///
    /// This value indicates the quality of the linear fit - smaller values
    /// indicate better synchronization. It's computed during the least-squares
    /// fitting process used to determine the gain and offset parameters.
    pub residuals: f64,

    /// Number of timestamp measurements used to compute this model.
    ///
    /// More measurements typically lead to better model accuracy.
    /// The synchronization system collects timestamp pairs over time
    /// to build a robust clock model.
    pub n_measurements: u64,
}
