use tokio_util::codec::{Decoder, Encoder};

use crate::{
    FlydraRawUdpPacket, FlydraRawUdpPacketHeader, ReadFlydraExt, FLYDRA1_PACKET_HEADER_SIZE,
    FLYDRA1_PER_POINT_PAYLOAD_SIZE,
};

pub struct FlydraPacketCodec {
    current_header: Option<FlydraRawUdpPacketHeader>,
}

impl Default for FlydraPacketCodec {
    fn default() -> Self {
        Self {
            current_header: None,
        }
    }
}

impl Decoder for FlydraPacketCodec {
    type Item = FlydraRawUdpPacket;
    type Error = std::io::Error;

    fn decode(
        &mut self,
        buf: &mut bytes::BytesMut,
    ) -> std::result::Result<Option<Self::Item>, Self::Error> {
        if self.current_header.is_none() {
            if buf.len() < FLYDRA1_PACKET_HEADER_SIZE {
                return Ok(None);
            }
            let header_bytes = buf.split_to(FLYDRA1_PACKET_HEADER_SIZE);
            let mut b_read = std::io::BufReader::new(&header_bytes[..]);
            self.current_header =
                Some(b_read.read_header().map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", e))
                })?);
        }

        // If we are here, self.current_header is not None.
        if let Some(header) = self.current_header.take() {
            let payload_size = FLYDRA1_PER_POINT_PAYLOAD_SIZE * header.len_points as usize;
            if buf.len() < payload_size {
                self.current_header = Some(header);
                return Ok(None);
            }

            let points_bytes = buf.split_to(payload_size);
            let mut b_read = std::io::BufReader::new(&points_bytes[..]);
            let points = b_read
                .read_points(header.len_points)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", e)))?;
            let packet = FlydraRawUdpPacket::from_header_and_points(header, points);
            debug_assert!(buf.len() <= FLYDRA1_PACKET_HEADER_SIZE);
            Ok(Some(packet))
        } else {
            panic!("unreachable"); // once we verify this is never reached, change to `unreachable!();`
        }
    }
}

impl Encoder for FlydraPacketCodec {
    type Item = ();
    type Error = std::io::Error;

    fn encode(&mut self, _item: (), _dest: &mut bytes::BytesMut) -> std::io::Result<()> {
        todo!();
    }
}
