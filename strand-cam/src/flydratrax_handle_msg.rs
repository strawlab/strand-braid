use tokio::sync::mpsc::UnboundedSender;

use crate::*;

use flydra2::{SendKalmanEstimatesRow, SendType};

#[cfg(feature = "with_led_box")]
use strand_cam_storetype::LedProgramConfig;

pub(crate) struct FlydraTraxServer {
    model_sender: UnboundedSender<SendType>,
}

impl FlydraTraxServer {
    pub(crate) fn new(model_sender: UnboundedSender<SendType>) -> Self {
        Self { model_sender }
    }
}

impl flydra2::GetsUpdates for FlydraTraxServer {
    fn send_update(
        &self,
        msg: SendType,
        _tdpt: &flydra2::TimeDataPassthrough,
    ) -> std::result::Result<(), flydra2::Error> {
        self.model_sender
            .send(msg)
            .map_err(|e| flydra2::wrap_error(e))?;
        Ok(())
    }
}

pub async fn flydratrax_handle_msg(
    cam_cal: mvg::Camera<MyFloat>,
    mut model_receiver: tokio::sync::mpsc::UnboundedReceiver<flydra2::SendType>,
    #[allow(unused_variables)] led_state: &mut bool,
    #[allow(unused_variables)] ssa2: Arc<RwLock<ChangeTracker<StoreType>>>,
    #[allow(unused_variables)] led_box_tx_std: tokio::sync::mpsc::Sender<ToLedBoxDevice>,
) -> Result<()> {
    use mvg::PointWorldFrame;
    use na::Point3;

    info!("starting new flydratrax_handle_msg");

    let mut cur_pos2d: Option<(u32, mvg::DistortedPixel<f64>)> = None;

    loop {
        let msg = match model_receiver.recv().await {
            Some(msg) => msg,
            None => break, // sender hung up - we are done.
        };
        debug!("got model msg: {:?}", msg);

        match msg {
            SendType::Update(row) | SendType::Birth(row) => {
                let record: SendKalmanEstimatesRow = row; // type annotation
                let pt3d = PointWorldFrame {
                    coords: Point3::new(record.x, record.y, record.z),
                };
                let pt2d = cam_cal.project_3d_to_distorted_pixel(&pt3d);

                // It is a bit strange to go back from 3D->2D coords, but the tracking
                // is done in 3D, so here we take the tracking data and put back in 2D.

                let next: Option<(u32, mvg::DistortedPixel<f64>)> = match cur_pos2d {
                    None => {
                        // We are not tracking anything yet, track this obj_id.
                        Some((record.obj_id, pt2d))
                    }
                    Some((cur_obj_id, cur_pt2d)) => {
                        if cur_obj_id == record.obj_id {
                            // Update our current position.
                            Some((cur_obj_id, pt2d))
                        } else {
                            // This is an update to a different object, do not update.
                            Some((cur_obj_id, cur_pt2d))
                        }
                    }
                };
                cur_pos2d = next;
            }
            SendType::Death(obj_id) => {
                let next = match cur_pos2d {
                    None => {
                        // We are not tracking anything yet, do nothing.
                        None
                    }
                    Some((cur_obj_id, cur_pt2d)) => {
                        if obj_id == cur_obj_id {
                            // The object we were tracking is now dead, stop tracking it.
                            None
                        } else {
                            // Some other object died. Carry on.
                            Some((cur_obj_id, cur_pt2d))
                        }
                    }
                };
                cur_pos2d = next;
            }
            SendType::EndOfFrame(_fno) => {}
        }

        #[cfg(feature = "with_led_box")]
        {
            let led_program_config: LedProgramConfig = {
                let store = ssa2.read();
                store.as_ref().led_program_config.clone()
            };
            let led_trigger_mode = led_program_config.led_trigger_mode;

            let (led_center, led_radius_raw) = match led_program_config.led_on_shape_pixels {
                video_streaming::Shape::Polygon(ref _points) => {
                    unimplemented!();
                }
                video_streaming::Shape::Circle(ref circ) => (
                    na::Point2::new(circ.center_x as f64, circ.center_y as f64),
                    circ.radius as f64,
                ),
                video_streaming::Shape::Everything => {
                    // actually nothing
                    (na::Point2::new(0.0, 0.0), -1.0)
                }
            };

            let led_radius = if *led_state {
                // LED is on, fly must leave a larger area to turn off LED.
                led_radius_raw + led_program_config.led_hysteresis_pixels as f64
            } else {
                led_radius_raw
            };

            let obj_in_led_radius = match &cur_pos2d {
                None => false,
                Some((_cur_obj_id, cur_pt2d)) => {
                    let this_dist = na::distance(&cur_pt2d.coords, &led_center);
                    if this_dist <= led_radius {
                        true
                    } else {
                        false
                    }
                }
            };

            let next_led_state = match led_trigger_mode {
                strand_cam_storetype::LEDTriggerMode::Off => continue, // skip below, thus preventing LED state change
                strand_cam_storetype::LEDTriggerMode::PositionTriggered => obj_in_led_radius,
            };

            if *led_state != next_led_state {
                info!("switching LED to ON={:?}", next_led_state);
                let device_state: Option<led_box_comms::DeviceState> = {
                    let tracker = ssa2.read();
                    tracker.as_ref().led_box_device_state.clone()
                };
                if let Some(mut device_state) = device_state {
                    let on_state = match next_led_state {
                        true => led_box_comms::OnState::ConstantOn,
                        false => led_box_comms::OnState::Off,
                    };

                    match led_program_config.led_channel_num {
                        1 => {
                            device_state.ch1.on_state = on_state;
                        }
                        2 => {
                            device_state.ch2.on_state = on_state;
                        }
                        3 => {
                            device_state.ch3.on_state = on_state;
                        }
                        other => {
                            error!("unsupported LED channel: {:?}", other);
                        }
                    }
                    let msg = led_box_comms::ToDevice::DeviceState(device_state);
                    led_box_tx_std.send(msg).await.unwrap();
                }
                *led_state = next_led_state;
            }
        }
    }
    Ok(())
}
