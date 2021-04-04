use bytes::buf::Buf;
use tokio_util::codec::{Decoder, Encoder};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("CamtrigError {0}")]
    CamtrigError(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0:?}")]
    MiniRxTx(#[from] mini_rxtx::Error),
    #[error("{0}")]
    ParseInt(#[from] std::num::ParseIntError),
}

/// wrap a CamtrigCodec into ToDevice and FromDevice types
pub struct CamtrigCodec {
    send_buf: [u8; 128],
    decoder: mini_rxtx::StdDecoder,
}

impl CamtrigCodec {
    pub fn new() -> Self {
        Self {
            send_buf: [0; 128],
            decoder: mini_rxtx::StdDecoder::new(256),
        }
    }
}

impl Decoder for CamtrigCodec {
    type Item = camtrig_comms::FromDevice;
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

impl Encoder<camtrig_comms::ToDevice> for CamtrigCodec {
    type Error = Error;

    fn encode(&mut self, msg: camtrig_comms::ToDevice, buf: &mut bytes::BytesMut) -> Result<()> {
        let serialized_msg =
            mini_rxtx::serialize_msg(&msg, &mut self.send_buf).expect("serialize_msg");
        buf.extend_from_slice(serialized_msg.framed_slice());
        Ok(())
    }
}
