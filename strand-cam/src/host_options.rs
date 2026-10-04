// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Per-camera integration channels supplied by an embedding application.
//!
//! Strand Camera can run as a library inside another application — FLO is the
//! motivating case. Everything such a host wires up for one camera lives here:
//! camera control and an optional frame sink. None of it involves networking; an
//! embedded host talks to Strand Camera over local channels.
//!
//! Detection is deliberately absent. Strand Camera's own ImOps detector serves
//! the standalone browser-UI-plus-UDP deployment; an embedding host reads the
//! frame sink, runs whatever detector it wants, and may send the results back
//! for the preview via `annotation_rx`.

/// Per-camera integration channels supplied by an embedding application.
///
/// `cam_args_rx`, when present, is a bounded Tokio channel from the host to
/// Strand Camera. Its [`strand_cam_remote_control::CamArg`] values are routed
/// through the same command task as `CallbackType::ToCamera` HTTP requests.
/// Closing this channel only disables host-side camera control; it does not
/// stop camera acquisition.
///
/// `frame_sink`, when present, receives every acquired frame so the host can do
/// its own per-frame work — detection included. Delivery is lossless: Strand
/// Camera *waits* for capacity, so a host that falls behind slows frame
/// processing instead of missing frames. Keep the channel shallow and drain it
/// promptly. See [`crate::host_frame_sink`].
///
/// `annotation_rx`, when present, is the return leg: whatever the host's own
/// detector found, drawn on the camera's browser preview. Latest-value only, so
/// a host that stops updating cannot back anything up. See
/// [`crate::host_annotation`].
pub struct StrandCamHostOptions {
    pub cam_args_rx: Option<tokio::sync::mpsc::Receiver<strand_cam_remote_control::CamArg>>,
    pub frame_sink: Option<tokio::sync::mpsc::Sender<crate::host_frame_sink::HostFrame>>,
    pub annotation_rx: Option<tokio::sync::watch::Receiver<crate::host_annotation::HostAnnotation>>,
}
