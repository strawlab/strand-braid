// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

/// Holds the most recent video frame (with annotations) received from the
/// server, to be consumed by [`VideoField`](crate::components::VideoField).
#[derive(PartialEq)]
pub struct VideoData {
    inner: Option<strand_http_video_streaming_types::ToClient>,
}

impl VideoData {
    pub fn new(inner: Option<strand_http_video_streaming_types::ToClient>) -> Self {
        Self { inner }
    }
    pub fn take(&mut self) -> Option<strand_http_video_streaming_types::ToClient> {
        self.inner.take()
    }
}
