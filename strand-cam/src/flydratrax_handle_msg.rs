use async_change_tracker::ChangeTracker;
use nalgebra as na;
use std::sync::{Arc, RwLock};
use tracing::{debug, error, info};

use crate::Result;
use braid_types::MyFloat;
use flydra2::{SendKalmanEstimatesRow, SendType};
use strand_cam_storetype::{LedProgramConfig, StoreType, ToLedBoxDevice};

// create a long-lived future that will process data from flydra and turn on
// LEDs with it.
pub async fn create_message_handler(
    cam_cal: mvg::Camera<MyFloat>,
    mut model_receiver: tokio::sync::mpsc::Receiver<(
        flydra2::SendType,
        flydra2::TimeDataPassthrough,
    )>,
    led_state: &mut bool,
    ssa2: Arc<RwLock<ChangeTracker<StoreType>>>,
    led_box_tx_std: tokio::sync::mpsc::Sender<ToLedBoxDevice>,
) -> Result<()> {
    use mvg::PointWorldFrame;
    use na::Point3;

    info!("starting new flydratrax message handler");

    let mut cur_pos2d: Option<(u32, mvg::DistortedPixel<f64>)> = None;

    loop {
        let full_msg = match model_receiver.recv().await {
            Some(full_msg) => full_msg,
            None => break, // sender hung up - we are done.
        };
        let (msg, _time_data_passthrough) = full_msg;
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
            SendType::CalibrationFlydraXml(_cal_xml) => {}
        }

        {
            let led_program_config: LedProgramConfig = {
                let store = ssa2.read().unwrap();
                store.as_ref().led_program_config.clone()
            };
            let led_trigger_mode = led_program_config.led_trigger_mode;

            if led_trigger_mode == strand_cam_storetype::LEDTriggerMode::Off {
                continue; // skip below, thus preventing LED state change
            }

            assert_eq!(
                led_trigger_mode,
                strand_cam_storetype::LEDTriggerMode::PositionTriggered
            );

            let circ_params = match led_program_config.led_on_shape_pixels {
                strand_http_video_streaming::Shape::Polygon(ref _points) => {
                    unimplemented!();
                }
                strand_http_video_streaming::Shape::MultipleCircles(ref circles) => {
                    circles.iter().map(|circ| to_circ_params(circ)).collect()
                }
                strand_http_video_streaming::Shape::Circle(ref circ) => {
                    vec![to_circ_params(circ)]
                }
                strand_http_video_streaming::Shape::Everything => {
                    // actually nothing
                    vec![]
                }
            };

            let mut next_led_state = false;

            for (led_center, led_radius_raw) in circ_params.iter() {
                let led_radius = if *led_state {
                    // LED is on, fly must leave a larger area to turn off LED.
                    led_radius_raw + led_program_config.led_hysteresis_pixels as f64
                } else {
                    *led_radius_raw
                };

                match &cur_pos2d {
                    None => {}
                    Some((_cur_obj_id, cur_pt2d)) => {
                        let this_dist = na::distance(&cur_pt2d.coords, &led_center);
                        if this_dist <= led_radius {
                            next_led_state = true;
                            break;
                        }
                    }
                };
            }

            if *led_state != next_led_state {
                info!("switching LED to ON={:?}", next_led_state);
                let device_state: Option<strand_led_box_comms::DeviceState> = {
                    let tracker = ssa2.read().unwrap();
                    tracker.as_ref().led_box_device_state.clone()
                };
                if let Some(mut device_state) = device_state {
                    let on_state = match next_led_state {
                        true => strand_led_box_comms::OnState::ConstantOn,
                        false => strand_led_box_comms::OnState::Off,
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
                        12 => {
                            device_state.ch1.on_state = on_state;
                            device_state.ch2.on_state = on_state;
                        }
                        13 => {
                            device_state.ch1.on_state = on_state;
                            device_state.ch3.on_state = on_state;
                        }
                        23 => {
                            device_state.ch2.on_state = on_state;
                            device_state.ch3.on_state = on_state;
                        }
                        123 => {
                            device_state.ch1.on_state = on_state;
                            device_state.ch2.on_state = on_state;
                            device_state.ch3.on_state = on_state;
                        }
                        other => {
                            error!("unsupported LED channel: {:?}", other);
                        }
                    }
                    let msg = strand_led_box_comms::ToDevice::DeviceState(device_state);
                    led_box_tx_std.send(msg).await.unwrap();
                }
                *led_state = next_led_state;
            }
        }
    }
    Ok(())
}

fn to_circ_params(
    circ: &strand_http_video_streaming_types::CircleParams,
) -> (na::Point2<f64>, f64) {
    (
        na::Point2::new(circ.center_x as f64, circ.center_y as f64),
        circ.radius as f64,
    )
}
