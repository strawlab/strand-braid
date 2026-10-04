// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What an embedding host found, offered back for the browser preview.
//!
//! This is the return leg of [`crate::host_frame_sink`]. A host that runs its
//! own detector on Strand Camera's frames can send the points back so the
//! camera's own preview still shows them, as it did when the detector lived
//! here.
//!
//! The marks are always retrospective. Strand Camera publishes frame *N* to the
//! preview before the host has finished looking at it, so the newest annotation
//! available when frame *N* goes out describes frame *N-1* at best. That is why
//! an annotation carries the identity of the frame it came from: the preview can
//! then say how far behind the marks are instead of implying they are current.
//!
//! Delivery is a [`tokio::sync::watch`] channel — latest value only, never a
//! queue. A host that skips an update loses nothing but a stale mark, and a
//! host that stops sending cannot back anything up.

use chrono::{DateTime, Utc};
use strand_http_video_streaming_types::Point;

/// Points an embedding host detected in one frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HostAnnotation {
    /// Where the host found things, in image pixel coordinates. Empty means
    /// "looked, found nothing" — which is worth sending, because it clears the
    /// previous mark.
    pub points: Vec<Point>,
    /// The [`crate::host_frame_sink::HostFrame::frame_number`] these points came
    /// from.
    pub frame_number: u64,
    /// That frame's [`crate::host_frame_sink::HostFrame::timestamp`].
    pub timestamp: Option<DateTime<Utc>>,
}
