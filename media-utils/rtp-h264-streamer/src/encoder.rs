// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::Result;

/// A pluggable H.264 encoder feeding one RTP stream.
///
/// Encoded access units are pushed to the `mpsc::SyncSender<h264_rtp::AccessUnit>`
/// the implementation was constructed with, not returned from [`Self::submit`]:
/// the openh264 implementation does this synchronously inside `submit` (one
/// `encode()` call is exactly one access unit), while the ffmpeg implementation
/// only writes raw rows to its child's stdin in `submit` and pushes access units
/// later, from its own stdout-reader thread. This lets both implementations
/// share one sender thread (RTP session state: SSRC, sequence counter, 90 kHz
/// timestamp base) that survives an encoder respawn on [`Self::set_bitrate`].
pub trait H264StreamEncoder: Send {
    /// Feed one frame to the encoder.
    fn submit(
        &mut self,
        frame: &strand_dynamic_frame::DynamicFrame,
        pts: std::time::Duration,
    ) -> Result<()>;

    /// Change the target bitrate. Takes effect on a best-effort basis: the
    /// openh264 implementation applies it to the running encoder immediately;
    /// the ffmpeg implementation respawns the child process, which loses a few
    /// frames but keeps the RTP session (and thus the receiver's decoder state)
    /// intact.
    fn set_bitrate(&mut self, bps: u32) -> Result<()>;

    /// Force the next encoded frame to be a keyframe (IDR).
    fn request_keyframe(&mut self) -> Result<()>;

    /// Flush and shut down the encoder. Consumes `self` because there is
    /// nothing meaningful left to do with an encoder afterward, and both
    /// implementations need to tear down owned resources (a child process, in
    /// the ffmpeg case) by value.
    fn finish(self: Box<Self>) -> Result<()>;
}
