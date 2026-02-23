use std::str;

use futures::{sink::SinkExt, stream::StreamExt};
use tokio_util::codec::Decoder;

use tokio_serial::SerialPortBuilderExt;

use tracing::info;

use clap::Parser;

use json_lines::codec::JsonLinesCodec;
use strand_led_box_comms::{ChannelState, DeviceState, OnState, ToDevice};

/// this handles the serial port and therefore the interaction with the device
async fn try_serial(serial_device: &str, next_state: &DeviceState) {
    info!(
        "opening serial port at {} baud. Using encoding '{}'",
        strand_led_box_comms::BAUD_RATE,
        "JSON + newlines"
    );
    #[allow(unused_mut)]
    let mut port = tokio_serial::new(serial_device, strand_led_box_comms::BAUD_RATE)
        .open_native_async()
        .unwrap();

    #[cfg(unix)]
    port.set_exclusive(false)
        .expect("Unable to set serial port exclusive to false");

    let (mut writer, mut reader) = JsonLinesCodec::default().framed(port).split();

    let msg = ToDevice::DeviceState(*next_state);
    info!("sending: {:?}", msg);
    writer.send(msg).await.unwrap();

    let start = std::time::Instant::now();

    // Create a stream to call our closure every second.
    let mut interval_stream = tokio::time::interval(std::time::Duration::from_millis(1000));

    let stream_future = async move {
        loop {
            tokio::select! {
                _instant = interval_stream.tick() => {
                    // This is called once a second.

                    let now = start.elapsed();
                    let now_tenth_millis: u64 = (now.as_micros() / 100 % (u64::MAX as u128)).try_into().unwrap();
                    let d = now_tenth_millis.to_le_bytes();
                    let msg = ToDevice::EchoRequest8((d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]));
                    info!("sending: {:?}", msg);

                    writer.send(msg).await.unwrap();
                }
                msg = reader.next() => {
                    match msg {
                        Some(Ok(strand_led_box_comms::FromDevice::EchoResponse8(d))) => {
                            let buf = [d.0, d.1, d.2, d.3, d.4, d.5, d.6, d.7];
                            let sent_tenth_millis: u64 = u64::from_le_bytes(buf);
                            let now = start.elapsed();
                            let now_tenth_millis: u64 =
                                (now.as_micros() / 100 % (u64::MAX as u128)).try_into().unwrap();
                            info!("round trip time: {} msec", (now_tenth_millis - sent_tenth_millis)as f64/10.0);
                        }
                        Some(Ok(strand_led_box_comms::FromDevice::VersionResponse(found))) => {
                            info!("Found comm version {found}.");
                            let expected = strand_led_box_comms::COMM_VERSION;
                            if found != expected {
                                tracing::error!("This program compiled to support comm version {expected}, but found version {found}.");
                                return;
                            }
                        }
                        Some(Ok(strand_led_box_comms::FromDevice::StateWasSet)) => {
                            info!("state was set");
                        }
                        Some(Ok(strand_led_box_comms::FromDevice::DeviceState(_))) => {}
                        Some(Err(e)) => {
                            panic!("unexpected error: {}: {:?}", e, e);
                        }
                        None => {
                            // sender hung up
                            break;
                        }
                    }
                }
            }
        }
    };

    stream_future.await;
}

fn make_chan(num: u8, on_state: OnState) -> ChannelState {
    let intensity = strand_led_box_comms::MAX_INTENSITY;
    ChannelState {
        num,
        intensity,
        on_state,
    }
}

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    device: String,
    #[arg(long)]
    all_leds_on: bool,
    #[arg(long)]
    all_leds_off: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("RUST_LOG", "info") };
    }
    env_tracing_logger::init();

    let cli = Cli::parse();

    let device_name = cli.device;

    let on_state = if cli.all_leds_on {
        if cli.all_leds_off {
            anyhow::bail!("cannot request LEDs both on and off");
        }
        OnState::ConstantOn
    } else {
        OnState::Off
    };

    let next_state = DeviceState {
        ch1: make_chan(1, on_state),
        ch2: make_chan(2, on_state),
        ch3: make_chan(3, on_state),
        ch4: make_chan(4, on_state),
    };

    try_serial(&device_name, &next_state).await;

    Ok(())
}
