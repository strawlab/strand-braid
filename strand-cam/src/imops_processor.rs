// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Transport-independent processing for the experimental FLO image detector.
//!
//! The caller owns both the frame source and the destination of a detection.
//! In particular, this module neither serializes results nor opens sockets.

use chrono::{DateTime, Utc};
use machine_vision_formats::{owned::OImage, pixel_format::Mono8};

use crate::TimestampSource;

/// Metadata identifying the acquisition of one image.
#[derive(Debug, PartialEq, Clone)]
pub struct ImOpsFrameMetadata {
    pub frame_number: u64,
    pub timestamp: DateTime<Utc>,
    pub timestamp_source: TimestampSource,
    pub camera_name: String,
}

/// Configuration for threshold-and-moment image processing.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct ImOpsProcessorConfig {
    pub threshold: u8,
    pub center_x: u32,
    pub center_y: u32,
}

/// Initial configuration for ImOps when Strand Camera is embedded in a host
/// application.
///
/// This deliberately contains no network configuration: an embedded host
/// receives detections over its local channel.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct ImOpsHostConfiguration {
    pub enabled: bool,
    pub processor: ImOpsProcessorConfig,
}

/// A point in image pixel coordinates.
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct ImagePoint {
    pub x: f32,
    pub y: f32,
}

/// The result of processing a single image.
#[derive(Debug, PartialEq, Clone)]
pub struct ImOpsDetection {
    pub metadata: ImOpsFrameMetadata,
    pub mu00: f32,
    pub mu01: f32,
    pub mu10: f32,
    pub center_x: u32,
    pub center_y: u32,
    /// `None` when no pixels survived thresholding.
    pub centroid: Option<ImagePoint>,
}

/// Local ImOps integration supplied by an embedding application.
///
/// `detection_tx` must be a bounded Tokio channel sender. Strand Camera uses
/// [`tokio::sync::mpsc::Sender::try_send`] and drops a new detection when the
/// channel is full, so slow host-side processing never stalls acquisition.
#[derive(Clone)]
pub struct ImOpsHostOptions {
    pub initial_configuration: ImOpsHostConfiguration,
    pub detection_tx: tokio::sync::mpsc::Sender<ImOpsDetection>,
}

/// Threshold a Mono8 image and calculate its spatial moments.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct ImOpsProcessor {
    config: ImOpsProcessorConfig,
}

impl ImOpsProcessor {
    pub fn new(config: ImOpsProcessorConfig) -> Self {
        Self { config }
    }

    /// Process an owned image. Pixels below `config.threshold` are excluded.
    pub fn process(&self, image: OImage<Mono8>, metadata: ImOpsFrameMetadata) -> ImOpsDetection {
        let thresholded =
            imops::threshold(image, imops::CmpOp::LessThan, self.config.threshold, 0, 255);
        let mu00 = imops::spatial_moment_00(&thresholded);
        let mu01 = imops::spatial_moment_01(&thresholded);
        let mu10 = imops::spatial_moment_10(&thresholded);
        let centroid = (mu00 != 0.0).then(|| ImagePoint {
            x: mu10 / mu00,
            y: mu01 / mu00,
        });

        ImOpsDetection {
            metadata,
            mu00,
            mu01,
            mu10,
            center_x: self.config.center_x,
            center_y: self.config.center_y,
            centroid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> ImOpsFrameMetadata {
        ImOpsFrameMetadata {
            frame_number: 42,
            timestamp: DateTime::UNIX_EPOCH,
            timestamp_source: TimestampSource::HostAcquiredTimestamp,
            camera_name: "camera-a".to_owned(),
        }
    }

    #[test]
    fn detects_centroid_and_preserves_metadata() {
        let image = OImage::<Mono8>::new(3, 2, 3, vec![0, 200, 100, 150, 0, 0]).unwrap();
        let processor = ImOpsProcessor::new(ImOpsProcessorConfig {
            threshold: 100,
            center_x: 12,
            center_y: 34,
        });

        let detection = processor.process(image, metadata());

        assert_eq!(detection.mu00, 765.0);
        assert_eq!(detection.mu01, 255.0);
        assert_eq!(detection.mu10, 765.0);
        assert_eq!(
            detection.centroid,
            Some(ImagePoint {
                x: 1.0,
                y: 1.0 / 3.0,
            })
        );
        assert_eq!(detection.center_x, 12);
        assert_eq!(detection.center_y, 34);
        assert_eq!(detection.metadata, metadata());
    }

    #[test]
    fn empty_threshold_result_has_no_centroid() {
        let image = OImage::<Mono8>::new(2, 1, 2, vec![1, 2]).unwrap();
        let processor = ImOpsProcessor::new(ImOpsProcessorConfig {
            threshold: 3,
            center_x: 0,
            center_y: 0,
        });

        let detection = processor.process(image, metadata());

        assert_eq!(detection.mu00, 0.0);
        assert_eq!(detection.centroid, None);
    }
}
