use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use tokio_serial::SerialPortBuilderExt;
use tokio_util::codec::Decoder;

use parking_lot::Mutex;

use log::{debug, error, info};

use led_box::LedBoxCodec;
use led_box_comms::{ChannelState, DeviceState, OnState, ToDevice};

#[derive(Debug, PartialEq, Clone)]
pub enum Cmd {
    Connect(String),
    Toggle(u8),
    Quit,
}

pub struct BoxManagerInner {
    to_box_writer: tokio::sync::mpsc::Sender<ToDevice>,
    state: DeviceState,
}

fn _test_box_manager_is_send() {
    // Compile-time test to ensure BoxManagerInner implements Send trait.
    fn implements<T: Send>() {}
    implements::<BoxManagerInner>();
}

pub struct BoxManager {
    inner: Option<BoxManagerInner>,
}

impl BoxManager {
    pub fn new() -> Self {
        Self { inner: None }
    }

    pub fn status(&self) -> BoxStatus {
        if let Some(inner) = &self.inner {
            BoxStatus::Connected(inner.state.clone())
        } else {
            BoxStatus::Unconnected
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum BoxStatus {
    Connected(DeviceState),
    Unconnected,
}

fn make_chan(num: u8, on_state: OnState) -> ChannelState {
    let intensity = led_box_comms::MAX_INTENSITY;
    ChannelState {
        num,
        intensity,
        on_state,
    }
}

pub async fn handle_box(
    mut box_manager: Arc<Mutex<BoxManager>>,
    mut cmd_rx: tokio::sync::mpsc::Receiver<Cmd>,
) -> anyhow::Result<()> {
    // initial state - unconnected
    assert_eq!(box_manager.lock().status(), BoxStatus::Unconnected);

    let device_name;
    loop {
        match cmd_rx.recv().await {
            Some(Cmd::Connect(port)) => {
                device_name = port;
                break;
            }
            Some(Cmd::Toggle(_chan)) => {
                panic!("Cannot toggle LED when not yet connected");
            }
            Some(Cmd::Quit) | None => {
                error!("exiting serial task before device opened");
                // quit request or channel closed
                return Ok(());
            }
        }
    }

    let next_state = DeviceState {
        ch1: make_chan(1, OnState::Off),
        ch2: make_chan(2, OnState::Off),
        ch3: make_chan(3, OnState::Off),
        ch4: make_chan(4, OnState::Off),
    };

    info!("connecting to {}", device_name);

    #[allow(unused_mut)]
    let mut port = tokio_serial::new(&device_name, 9600)
        .open_native_async()
        .unwrap();

    #[cfg(unix)]
    port.set_exclusive(false)
        .expect("Unable to set serial port exclusive to false");

    let (mut serial_writer, mut serial_reader) = LedBoxCodec::new().framed(port).split();

    // Clear potential initially present bytes from stream...
    let _ = tokio::time::timeout(std::time::Duration::from_millis(50), serial_reader.next()).await;

    let (to_box_writer, mut to_box_reader) = tokio::sync::mpsc::channel::<ToDevice>(20);

    let mpsc_to_serial = async move {
        loop {
            match to_box_reader.recv().await {
                Some(msg) => {
                    debug!("sending: {:?}", msg);
                    serial_writer.send(msg).await.unwrap();
                }
                None => panic!("sender hung up"),
            }
        }
    };
    tokio::spawn(mpsc_to_serial); // todo: keep join handle.

    to_box_writer
        .send(led_box_comms::ToDevice::VersionRequest)
        .await?;

    let msg = serial_reader.next().await.unwrap().unwrap();
    assert_eq!(
        msg,
        led_box_comms::FromDevice::VersionResponse(led_box_comms::COMM_VERSION)
    );
    info!(
        "Connected to firmware version {}",
        led_box_comms::COMM_VERSION
    );

    let msg = ToDevice::DeviceState(next_state);
    to_box_writer.send(msg).await.unwrap();

    {
        box_manager.lock().inner = Some(BoxManagerInner {
            to_box_writer,
            state: next_state,
        });
    }

    let start_led_box_instant = std::time::Instant::now();

    let printer = async move {
        while let Some(msg) = serial_reader.next().await {
            match msg {
                Ok(led_box_comms::FromDevice::EchoResponse8(d)) => {
                    let buf = [d.0, d.1, d.2, d.3, d.4, d.5, d.6, d.7];
                    let sent_millis: u64 = byteorder::ReadBytesExt::read_u64::<
                        byteorder::LittleEndian,
                    >(&mut std::io::Cursor::new(buf))
                    .unwrap();
                    let now = start_led_box_instant.elapsed();
                    let now_millis: u64 =
                        (now.as_millis() % (u64::MAX as u128)).try_into().unwrap();
                    debug!("round trip time: {} msec", now_millis - sent_millis);
                }
                Ok(msg) => {
                    error!("unknown message received: {:?}", msg);
                }
                Err(e) => {
                    panic!("unexpected error: {}: {:?}", e, e);
                }
            }
            debug!("received: {:?}", msg);
        }
    };
    tokio::spawn(printer);

    // Create a stream to call our closure every second.
    let mut interval_stream = tokio::time::interval(std::time::Duration::from_millis(1000));

    let stream_future = {
        let to_box_writer = box_manager
            .lock()
            .inner
            .as_ref()
            .unwrap()
            .to_box_writer
            .clone();
        async move {
            loop {
                interval_stream.tick().await;
                // This is called once a second.

                let now = start_led_box_instant.elapsed();
                let now_millis: u64 = (now.as_millis() % (u64::MAX as u128)).try_into().unwrap();
                let mut d = vec![];
                {
                    use byteorder::WriteBytesExt;
                    d.write_u64::<byteorder::LittleEndian>(now_millis).unwrap();
                }
                let msg = ToDevice::EchoRequest8((d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]));
                to_box_writer.send(msg).await.unwrap();
            }
        }
    };

    tokio::spawn(stream_future);

    // infinite loop to handle commands from UI
    loop {
        match cmd_rx.recv().await {
            Some(Cmd::Quit) | None => {
                // quit request or channel closed
                log::info!("exiting serial task");
                return Ok(());
            }
            Some(cmd) => {
                handle_cmd(cmd, &mut box_manager).await?;
            }
        }
    }
}

async fn handle_cmd(cmd: Cmd, box_manager: &mut Arc<Mutex<BoxManager>>) -> anyhow::Result<()> {
    match cmd {
        Cmd::Quit => {
            panic!("should handle quit outside this function");
        }
        Cmd::Connect(_) => {
            log::warn!("already connected");
        }
        Cmd::Toggle(chan) => {
            let mut guard = box_manager.lock();
            {
                let inner = guard.inner.as_mut().unwrap();
                {
                    let chan_ref = match chan {
                        1 => &mut inner.state.ch1,
                        2 => &mut inner.state.ch2,
                        3 => &mut inner.state.ch3,
                        4 => &mut inner.state.ch4,
                        other => {
                            panic!("unknown channel {}", other);
                        }
                    };
                    let next_on_state = match chan_ref.on_state {
                        OnState::ConstantOn => OnState::Off,
                        OnState::Off => OnState::ConstantOn,
                    };
                    chan_ref.on_state = next_on_state;
                }
                let msg = ToDevice::DeviceState(inner.state.clone());
                inner.to_box_writer.send(msg).await?
            }
        }
    }
    Ok(())
}
