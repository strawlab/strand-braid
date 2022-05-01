use bytes::buf::Buf;
use tokio_util::codec::{Decoder, Encoder};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("LedBoxError {0}")]
    LedBoxError(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0:?}")]
    MiniRxTx(#[from] mini_rxtx::Error),
    #[error("{0}")]
    ParseInt(#[from] std::num::ParseIntError),
}

/// wrap a LedBoxCodec into ToDevice and FromDevice types
pub struct LedBoxCodec {
    send_buf: [u8; 128],
    decoder: mini_rxtx::StdDecoder,
}

impl LedBoxCodec {
    pub fn new() -> Self {
        Self {
            send_buf: [0; 128],
            decoder: mini_rxtx::StdDecoder::new(256),
        }
    }
}

impl Decoder for LedBoxCodec {
    type Item = led_box_comms::FromDevice;
    type Error = Error;

    fn decode(&mut self, buf: &mut bytes::BytesMut) -> Result<Option<Self::Item>> {
        while buf.len() > 0 {
            let byte = buf[0];
            buf.advance(1);
            match self.decoder.consume::<Self::Item>(byte) {
                mini_rxtx::Decoded::Msg(msg) => {
                    return Ok(Some(msg));
                }
                mini_rxtx::Decoded::FrameNotYetComplete => {}
                mini_rxtx::Decoded::Error(e) => {
                    return Err(e.into());
                }
            }
        }
        Ok(None)
    }
}

impl Encoder<led_box_comms::ToDevice> for LedBoxCodec {
    type Error = Error;

    fn encode(&mut self, msg: led_box_comms::ToDevice, buf: &mut bytes::BytesMut) -> Result<()> {
        let serialized_msg =
            mini_rxtx::serialize_msg(&msg, &mut self.send_buf).expect("serialize_msg");
        buf.extend_from_slice(serialized_msg.framed_slice());
        Ok(())
    }
}
