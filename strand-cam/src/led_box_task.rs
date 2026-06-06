// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! LED box serial-device communication.
//!
//! This was extracted verbatim from the monolithic `run()` function in
//! `strand-cam.rs` to keep that function manageable. It opens the serial
//! connection to the LED box (if configured), verifies the firmware version,
//! and spawns the long-running tasks that pump messages to/from the device and
//! emit periodic heartbeats.

use std::sync::{Arc, RwLock};

use futures::{sink::SinkExt, stream::StreamExt};
use tracing::{debug, info};

use eyre::Result;

use async_change_tracker::ChangeTracker;
use strand_cam_storetype::{StoreType, ToLedBoxDevice};

const LED_BOX_HEARTBEAT_INTERVAL_MSEC: u64 = 5000;

/// Connect to the LED box (if a serial device path is configured) and spawn the
/// tasks that service it.
///
/// Returns once the connection has been established and the background tasks
/// have been spawned (the spawned tasks themselves run until the process
/// exits). If no LED box device path is configured this is a no-op.
pub(crate) async fn run_led_box_task(
    led_box_tx_std: tokio::sync::mpsc::Sender<ToLedBoxDevice>,
    mut led_box_rx: tokio::sync::mpsc::Receiver<ToLedBoxDevice>,
    led_box_heartbeat_update_arc: Arc<RwLock<Option<std::time::Instant>>>,
    shared_store_arc: Arc<RwLock<ChangeTracker<StoreType>>>,
) -> Result<()> {
    use tokio_serial::SerialPortBuilderExt;
    use tokio_util::codec::Decoder;

    use json_lines::codec::JsonLinesCodec;
    use strand_led_box_comms::{ChannelState, DeviceState, OnState};

    let start_led_box_instant = std::time::Instant::now();

    // enqueue initial message
    {
        fn make_chan(num: u8, on_state: OnState) -> ChannelState {
            let intensity = strand_led_box_comms::MAX_INTENSITY;
            ChannelState {
                num,
                intensity,
                on_state,
            }
        }

        let first_led_box_state = DeviceState {
            ch1: make_chan(1, OnState::Off),
            ch2: make_chan(2, OnState::Off),
            ch3: make_chan(3, OnState::Off),
            ch4: make_chan(4, OnState::Off),
        };

        led_box_tx_std
            .send(ToLedBoxDevice::DeviceState(first_led_box_state))
            .await
            .unwrap();
    }

    // open serial port
    let port = {
        let tracker = shared_store_arc.read().unwrap();
        let shared = tracker.as_ref();
        if let Some(serial_device) = shared.led_box_device_path.as_ref() {
            info!("opening LED box \"{}\"", serial_device);
            // open with default settings 9600 8N1
            let mut port = tokio_serial::new(serial_device, strand_led_box_comms::BAUD_RATE)
                .open_native_async()
                .unwrap();

            #[cfg(unix)]
            port.set_exclusive(false)
                .expect("Unable to set serial port exclusive to false");
            Some(port)
        } else {
            None
        }
    };

    if let Some(port) = port {
        // wrap port with codec
        let (mut writer, mut reader) = JsonLinesCodec::default().framed(port).split();

        // Clear potential initially present bytes from stream...
        let _ = tokio::time::timeout(std::time::Duration::from_millis(50), reader.next()).await;

        writer
            .send(strand_led_box_comms::ToDevice::VersionRequest)
            .await?;

        match tokio::time::timeout(std::time::Duration::from_millis(50), reader.next()).await {
            Ok(Some(Ok(msg))) => match msg {
                strand_led_box_comms::FromDevice::VersionResponse(
                    strand_led_box_comms::COMM_VERSION,
                ) => {
                    info!(
                        "Connected to firmware version {}",
                        strand_led_box_comms::COMM_VERSION
                    );
                }
                msg => {
                    eyre::bail!(
                        "Unexpected response from LED Box {:?}. Is your firmware version correct? (Needed version: {})",
                        msg,
                        strand_led_box_comms::COMM_VERSION
                    );
                }
            },
            Err(_elapsed) => {
                eyre::bail!(
                    "Timeout connecting to LED Box. Is your firmware version correct? (Needed version: {})",
                    strand_led_box_comms::COMM_VERSION
                );
            }
            Ok(None) | Ok(Some(Err(_))) => {
                eyre::bail!(
                    "Failed connecting to LED Box. Is your firmware version correct? (Needed version: {})",
                    strand_led_box_comms::COMM_VERSION
                );
            }
        }

        // handle messages from the device
        let from_device_task = async move {
            debug!("awaiting message from LED box");
            while let Some(msg) = tokio_stream::StreamExt::next(&mut reader).await {
                match msg {
                    Ok(strand_led_box_comms::FromDevice::EchoResponse8(d)) => {
                        let buf = [d.0, d.1, d.2, d.3, d.4, d.5, d.6, d.7];
                        let sent_millis: u64 =
                            byteorder::ReadBytesExt::read_u64::<byteorder::LittleEndian>(
                                &mut std::io::Cursor::new(buf),
                            )
                            .unwrap();

                        let now = start_led_box_instant.elapsed();
                        let now_millis: u64 =
                            (now.as_millis() % (u64::MAX as u128)).try_into().unwrap();
                        debug!("LED box round trip time: {} msec", now_millis - sent_millis);

                        // elsewhere check if this happens every LED_BOX_HEARTBEAT_INTERVAL_MSEC or so.
                        let mut led_box_heartbeat_update =
                            led_box_heartbeat_update_arc.write().unwrap();
                        *led_box_heartbeat_update = Some(std::time::Instant::now());
                    }
                    Ok(strand_led_box_comms::FromDevice::StateWasSet) => {}
                    Ok(msg) => {
                        todo!("Did not handle {:?}", msg);
                        // error!("unknown message received: {:?}", msg);
                    }
                    Err(e) => {
                        panic!("unexpected error: {e}: {e:?}");
                    }
                }
            }
        };
        tokio::spawn(from_device_task); // todo: keep join handle

        // handle messages to the device
        let to_device_task = async move {
            while let Some(msg) = led_box_rx.recv().await {
                // send message to device
                writer.send(msg).await.unwrap();
                // copy new device state and store it to our cache
                if let ToLedBoxDevice::DeviceState(new_state) = msg {
                    let mut tracker = shared_store_arc.write().unwrap();
                    tracker.modify(|shared| {
                        shared.led_box_device_state = Some(new_state);
                    })
                };
            }
        };
        tokio::spawn(to_device_task); // todo: keep join handle

        // heartbeat task
        let heartbeat_task = async move {
            let mut interval_stream = tokio::time::interval(std::time::Duration::from_millis(
                LED_BOX_HEARTBEAT_INTERVAL_MSEC,
            ));
            loop {
                interval_stream.tick().await;

                let now = start_led_box_instant.elapsed();
                let now_millis: u64 = (now.as_millis() % (u64::MAX as u128)).try_into().unwrap();
                let mut d = vec![];
                {
                    use byteorder::WriteBytesExt;
                    d.write_u64::<byteorder::LittleEndian>(now_millis).unwrap();
                }
                let msg =
                    ToLedBoxDevice::EchoRequest8((d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]));
                debug!("sending: {:?}", msg);

                led_box_tx_std.send(msg).await.unwrap();
            }
        };
        tokio::spawn(heartbeat_task); // todo: keep join handle
    }

    Ok(())
}
