// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every acquired frame, handed to an embedding application.
//!
//! This exists for hosts that do their own per-frame work — FLO, which runs its
//! object detector on the tracking camera and separately relays those frames to
//! a viewer process.
//!
//! Delivery is **lossless**: the sink is a bounded channel written with
//! [`tokio::sync::mpsc::Sender::send`], so a host that falls behind applies
//! backpressure to frame processing rather than silently missing a detection. A
//! host that only wants to *display* frames must therefore drop them itself
//! once it has received them; it must not simply stop reading.
//!
//! That backpressure is not where an overloaded system ends up losing frames.
//! `cam_stream_task` already drops a frame — loudly, with an error and a UI flag
//! — when the frame-processing queue is full. Making this sink lossless moves
//! the loss out of the detection path and back to that one existing, visible
//! choke point.
//!
//! The frame itself is handed over by `Arc` clone, with no copy and no
//! conversion. Keep the channel shallow: it should absorb scheduling jitter, not
//! build a backlog. A deep queue here only adds tracking latency and holds onto
//! several megabytes per camera.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use strand_dynamic_frame::DynamicFrameOwned;

use crate::TimestampSource;

/// One acquired frame handed to an embedding host.
///
/// The image is shared, not copied. Deliberately a distinct type from the
/// browser preview's `AnnotatedFrame`: a host wants the pixels and their
/// identity, not the web UI's annotation and display-validity fields.
///
/// There is no camera name here. The sink is registered per camera, so the host
/// already knows which one it is reading, and carrying the name would mean an
/// allocation per frame.
#[derive(Clone)]
pub struct HostFrame {
    /// The frame as acquired, in the camera's own pixel format.
    pub image: Arc<DynamicFrameOwned>,
    /// Host-counted frame number, as in [`ci2::HostTimingInfo::fno`].
    pub frame_number: u64,
    /// The most acquisition-faithful timestamp available for this frame: the
    /// trigger timestamp when one exists, otherwise the host grab time. This is
    /// the same stamp Strand Camera records to disk for the frame.
    pub timestamp: DateTime<Utc>,
    /// Which of those two [`Self::timestamp`] actually is.
    pub timestamp_source: TimestampSource,
}
