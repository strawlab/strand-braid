#[cfg(feature = "with-tokio-codec")]
use tokio_util::codec::{Decoder, Encoder};

use crate::{
    FlydraFloatTimestampLocal, FlydraRawUdpPacket, FlydraRawUdpPoint, HostClock, Triggerbox,
};

pub struct CborPacketCodec {
    buffered_results: std::collections::VecDeque<FlydraRawUdpPacket>,
}

impl Default for CborPacketCodec {
    fn default() -> Self {
        Self {
            buffered_results: std::collections::VecDeque::new(),
        }
    }
}

impl Decoder for CborPacketCodec {
    type Item = FlydraRawUdpPacket;
    type Error = std::io::Error;

    fn decode(
        &mut self,
        buf: &mut bytes::BytesMut,
    ) -> std::result::Result<Option<Self::Item>, Self::Error> {
        // TODO: Right now this is somewhat inefficient. The easier fix would be to add support
        // for decoding from the `bytes` crate in serde_cbor.

        // TODO: FIXME: This assumes that boundaries of buf fall on decode boundaries.

        // Parse all available input data.
        let available = buf.split();
        let deserializer = serde_cbor::Deserializer::from_slice(&available[..]);
        let new_results: Vec<Result<FlydraRawUdpPacket, serde_cbor::error::Error>> =
            deserializer.into_iter().collect();

        // early return on error
        let new_results: Result<Vec<FlydraRawUdpPacket>, serde_cbor::error::Error> =
            new_results.into_iter().collect();
        let new_results = match new_results {
            Ok(v) => v,
            Err(e) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("serde_cbor::error::Error {:?}", e),
                ));
            }
        };

        self.buffered_results.extend(new_results.into_iter());

        Ok(self.buffered_results.pop_front())
    }
}

#[cfg(feature = "with-tokio-codec")]
impl Encoder<FlydraRawUdpPacket> for CborPacketCodec {
    type Error = std::io::Error;

    fn encode(
        &mut self,
        item: FlydraRawUdpPacket,
        dest: &mut bytes::BytesMut,
    ) -> std::io::Result<()> {
        let item_bytes = serde_cbor::to_vec(&item).unwrap();
        dest.extend(item_bytes); // If dest does not have enough capacity, it is resized first.
        Ok(())
    }
}

// tests below here ---------------------

#[test]
fn cbor_decoder() {
    use bytes::{BufMut, BytesMut};

    let p1 = make_test_packet(1);
    let p1_bytes = serde_cbor::to_vec(&p1).unwrap();

    let p2 = make_test_packet(2);
    let p2_bytes = serde_cbor::to_vec(&p2).unwrap();

    let p1234 = make_test_packet(1234);
    let p1234_bytes = serde_cbor::to_vec(&p1234).unwrap();

    let mut codec = CborPacketCodec::default();
    let buf = &mut BytesMut::new();
    buf.reserve(2000);
    buf.put_slice(&p1_bytes);
    buf.put_slice(&p2_bytes);
    buf.put_slice(&p1234_bytes);

    assert_eq!(p1, codec.decode(buf).unwrap().unwrap());
    assert_eq!(p2, codec.decode(buf).unwrap().unwrap());
    assert_eq!(p1234, codec.decode(buf).unwrap().unwrap());
    assert_eq!(None, codec.decode(buf).unwrap());
    assert_eq!(None, codec.decode_eof(buf).unwrap());
    let p2_bytes = serde_cbor::to_vec(&p2).unwrap();
    buf.put_slice(&p2_bytes);
    assert_eq!(p2, codec.decode(buf).unwrap().unwrap());
    assert_eq!(None, codec.decode(buf).unwrap());
    assert_eq!(None, codec.decode_eof(buf).unwrap());
}

#[test]
fn cbor_roundtrip() {
    use bytes::BytesMut;

    let p1234 = make_test_packet(1234);

    let mut codec = CborPacketCodec::default();
    let mut buf = BytesMut::new();

    codec.encode(p1234.clone(), &mut buf).unwrap();
    assert_eq!(p1234, codec.decode(&mut buf).unwrap().unwrap());
}

#[allow(dead_code)]
fn make_test_packet(framenumber: i32) -> FlydraRawUdpPacket {
    use crate::ImageProcessingSteps;

    let cam_name = "cam_id".to_string();
    let timestamp = 12.34;
    let timestamp = Some(FlydraFloatTimestampLocal::<Triggerbox>::from_f64(timestamp));

    let cam_received_time = FlydraFloatTimestampLocal::<HostClock>::from_f64(123.456);
    let n_frames_skipped = 6;

    let points: Vec<FlydraRawUdpPoint> = vec![];

    FlydraRawUdpPacket {
        cam_name,
        timestamp,
        cam_received_time,
        framenumber,
        n_frames_skipped,
        done_camnode_processing: 0.0,
        preprocess_stamp: 0.0,
        image_processing_steps: ImageProcessingSteps::empty(),
        points,
    }
}
