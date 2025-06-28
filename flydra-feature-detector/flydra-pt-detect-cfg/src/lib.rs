// Copyright 2020-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use flydra_feature_detector_types::{ContrastPolarity, ImPtDetectCfg};
use strand_http_video_streaming_types::Shape;

fn my_default(polarity: ContrastPolarity, valid_region: Shape) -> ImPtDetectCfg {
    ImPtDetectCfg {
        do_update_background_model: true,
        polarity,
        alpha: 0.01,
        n_sigma: 7.0,
        bright_non_gaussian_cutoff: 255,
        bright_non_gaussian_replacement: 5,
        bg_update_interval: 200,
        diff_threshold: 30,
        use_cmp: true,
        max_num_points: 1,
        feature_window_size: 30,
        clear_fraction: 0.3,
        despeckle_threshold: 5,
        valid_region,
    }
}

pub fn default_absdiff() -> ImPtDetectCfg {
    my_default(ContrastPolarity::DetectAbsDiff, Shape::Everything)
}

pub fn default_dark_circle() -> ImPtDetectCfg {
    my_default(
        ContrastPolarity::DetectDark,
        Shape::Circle(strand_http_video_streaming_types::CircleParams {
            center_x: 640,
            center_y: 512,
            radius: 512,
        }),
    )
}
