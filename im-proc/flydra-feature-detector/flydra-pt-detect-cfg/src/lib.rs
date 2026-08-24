// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Default values for the [`ImPtDetectCfg`] type of the
//! [`flydra-feature-detector-types`](https://crates.io/crates/flydra-feature-detector-types)
//! crate.

use flydra_feature_detector_types::{ContrastPolarity, ImPtDetectCfg};
use strand_http_video_streaming_types::Shape;

fn my_default(polarity: ContrastPolarity, valid_region: Shape) -> ImPtDetectCfg {
    // The field values live in `ImPtDetectCfg::default`, which is also what
    // serde fills in for omitted fields. Keep them in that one place.
    ImPtDetectCfg {
        polarity,
        valid_region,
        ..Default::default()
    }
}

/// Default configuration for detecting features brighter or darker than background
pub fn default_absdiff() -> ImPtDetectCfg {
    my_default(ContrastPolarity::DetectAbsDiff, Shape::Everything)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_absdiff_matches_the_serde_defaults() {
        // Guards against these two notions of "default" drifting apart: the
        // values serde fills in for omitted fields must be the ones Braid and
        // Strand Camera start from.
        assert_eq!(default_absdiff(), ImPtDetectCfg::default());
    }
}
