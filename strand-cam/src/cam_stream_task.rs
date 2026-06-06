// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Camera frame-stream pump.
//!
//! Extracted from the monolithic `run()` function in `strand-cam.rs`. This
//! consumes the camera's frame stream: it forwards each frame to the
//! frame-processing task, optionally hands frames to the local eframe GUI, and
//! periodically pushes a PNG snapshot of the current image up to Braid.

use std::sync::{Arc, RwLock};

use futures::stream::StreamExt;
use tracing::{debug, error, trace};

use eyre::Result;

use async_change_tracker::ChangeTracker;
use strand_cam_storetype::StoreType;
use strand_dynamic_frame::DynamicFrame;

use crate::{FrameProcessingErrorState, Msg, to_eyre};

/// Drive the camera frame stream until it ends.
#[expect(
    clippy::too_many_arguments,
    reason = "extracted verbatim from run(); grouping these into a context struct is left for a later cleanup"
)]
pub(crate) async fn run_cam_stream_task(
    mut frame_stream: Box<dyn futures::Stream<Item = ci2_async::FrameResult> + Send + Unpin>,
    tx_frame: tokio::sync::mpsc::Sender<Msg>,
    shared_store_arc: Arc<RwLock<ChangeTracker<StoreType>>>,
    frame_processing_error_state: Arc<RwLock<FrameProcessingErrorState>>,
    mut transmit_msg_tx: Option<tokio::sync::mpsc::Sender<braid_types::BraidHttpApiCallback>>,
    raw_cam_name: braid_types::RawCamName,
    send_image_to_braid_interval: Option<std::time::Duration>,
    #[cfg(feature = "eframe-gui")] gui_stuff2: Option<(
        tokio::sync::watch::Sender<crate::gui_app::ImType>,
        eframe::egui::Context,
    )>,
    #[cfg(not(feature = "eframe-gui"))] gui_stuff2: Option<()>,
) -> Result<()> {
    let mut send_image_to_braid_timer = std::time::Instant::now();
    let mut send_image_to_braid_duration = std::time::Duration::from_millis(0);
    while let Some(frame_msg) = frame_stream.next().await {
        match &frame_msg {
            ci2_async::FrameResult::Frame(fframe) => {
                {
                    let frame: &DynamicFrame = &fframe.image.borrow();
                    trace!(
                        "  got frame {}: {}x{}",
                        fframe.host_timing.fno,
                        frame.width(),
                        frame.height()
                    );
                }

                #[cfg(not(feature = "eframe-gui"))]
                let _ = gui_stuff2.as_ref();

                #[cfg(feature = "eframe-gui")]
                {
                    if let Some((gui_frame_tx, egui_ctx)) = gui_stuff2.as_ref() {
                        let arc_clone = fframe.image.clone(); // copy pointer and increment refcount

                        match gui_frame_tx.send(arc_clone) {
                            Ok(()) => {
                                egui_ctx.request_repaint();
                            }
                            Err(_arc_clone) => {
                                eyre::bail!("GUI disconnected");
                            }
                        }
                    }
                }

                if tx_frame.capacity() == 0 {
                    let mut tracker = shared_store_arc.write().unwrap();
                    tracker.modify(|tracker| {
                        let mut state = frame_processing_error_state.write().unwrap();
                        {
                            match &*state {
                                FrameProcessingErrorState::IgnoreAll => {}
                                FrameProcessingErrorState::IgnoreUntil(ignore_until) => {
                                    let now = chrono::Utc::now();
                                    if now >= *ignore_until {
                                        tracker.had_frame_processing_error = true;
                                        *state = FrameProcessingErrorState::NotifyAll;
                                    }
                                }
                                FrameProcessingErrorState::NotifyAll => {
                                    tracker.had_frame_processing_error = true;
                                }
                            }
                        }
                    });
                    error!("Channel full sending frame to process thread. Dropping frame data.");
                } else {
                    tx_frame
                        .send(Msg::Mframe(fframe.clone()))
                        .await
                        .map_err(to_eyre)?;
                }
            }
            ci2_async::FrameResult::SingleFrameError(s) => {
                error!("SingleFrameError({})", s);
            }
        }

        if let ci2_async::FrameResult::Frame(frame) = &frame_msg
            && let Some(transmit_msg_tx) = transmit_msg_tx.as_mut()
        {
            // Check if we need to send this frame to braid because our timer elapsed.
            if send_image_to_braid_timer.elapsed() >= send_image_to_braid_duration {
                // If yes, encode frame to png buffer.
                let current_image_png = frame
                    .image
                    .borrow()
                    .to_encoded_buffer(convert_image::EncoderOptions::Png)
                    .unwrap();

                // Prepare and send message to Braid.
                let msg =
                    braid_types::BraidHttpApiCallback::UpdateCurrentImage(braid_types::PerCam {
                        raw_cam_name: raw_cam_name.clone(),
                        inner: braid_types::UpdateImage {
                            current_image_png: current_image_png.into(),
                        },
                    });
                transmit_msg_tx.send(msg).await?;

                // Update timer for next iteration.
                send_image_to_braid_timer = std::time::Instant::now();
                if let Some(dur) = send_image_to_braid_interval {
                    send_image_to_braid_duration = dur;
                }
            }
        }
    }
    debug!("cam_stream_future future done {}:{}", file!(), line!());

    Ok(())
}
