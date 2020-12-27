use std::str;

use futures::{stream::StreamExt, sink::SinkExt};
use tokio_util::codec::Decoder;

use camtrig::CamtrigCodec;
use camtrig::{Result,Error};
use camtrig_comms::{ToDevice, DeviceState, TriggerState, Running, OnState, ChannelState};

/// this handles the serial port and therefore the interaction with the device
async fn try_serial(serial_device: &str, next_state: &DeviceState) {
    let settings = tokio_serial::SerialPortSettings::default();

    let mut port = tokio_serial::Serial::from_path(serial_device, &settings).unwrap();
    mio_serial::SerialPort::set_baud_rate(&mut port, 9600).unwrap();

    #[cfg(unix)]
    port.set_exclusive(false)
        .expect("Unable to set serial port exlusive");

    let (mut writer, mut reader) = CamtrigCodec::new().framed(port).split();

    let msg = ToDevice::DeviceState(*next_state);
    println!("sending: {:?}", msg);
    writer.send(msg).await.unwrap();

    let printer = async move {
        while let Some(msg) = reader.next().await {
            println!("received: {:?}", msg);
        }
    };
    tokio::spawn(printer);

    // Create a stream to call our closure every second.
    let mut interval_stream =
        tokio::time::interval(std::time::Duration::from_millis(1000));

    let stream_future = async move {
        loop {
            interval_stream.tick().await;
            // This closure is called once a second.

            // let msg = ToDevice::EchoRequest8((1,2,3,4,5,6,7,8));
            // let msg = ToDevice::CounterInfoRequest(1);
            let msg = ToDevice::TimerRequest;
            println!("sending: {:?}", msg);

            writer.send(msg).await.unwrap();
        }
    };

    stream_future.await;
}

fn make_chan(num: u8, on_state: OnState) -> ChannelState {
    let intensity = camtrig_comms::MAX_INTENSITY;
    ChannelState { num, intensity, on_state }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let matches = clap::App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .arg(clap::Arg::with_name("freq")
                 .long("freq")
                 .takes_value(true))
        .arg(clap::Arg::with_name("device")
                 .long("device")
                 .takes_value(true))
        .arg(clap::Arg::with_name("all_leds_on")
                 .long("all-leds-on"))
        .get_matches();

    let freq_str =
        matches
            .value_of("freq")
            .ok_or(Error::CamtrigError("expected freq".into()))?;
    let freq_hz: u16 = freq_str.parse()?;
    println!("freq_hz: {}", freq_hz);

    let device_name =
        matches
            .value_of("device")
            .ok_or(Error::CamtrigError("expected device".into()))?;

    let running = match freq_hz {
        0 => Running::Stopped,
        f => Running::ConstantFreq(f)
    };

    let on_state = if matches.occurrences_of("all_leds_on") > 0 {
        OnState::ConstantOn
    } else {
        OnState::Off
    };

    let next_state = DeviceState {
        trig: TriggerState { running: running },
        ch1: make_chan(1, on_state),
        ch2: make_chan(2, on_state),
        ch3: make_chan(3, on_state),
        ch4: make_chan(4, on_state),
    };

    try_serial(device_name, &next_state).await;
    Ok(())
}
