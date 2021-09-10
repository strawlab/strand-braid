use super::{
    FlydraFloatTimestampLocal, FlydraRawUdpPacket, FlydraRawUdpPacketHeader, FlydraRawUdpPoint,
    FlydraTypesError, HostClock, ImageProcessingSteps, Result, Triggerbox,
};
use byteorder::{LittleEndian, WriteBytesExt};

pub const CAM_NAME_LEN: usize = 30;
pub const FLYDRA1_PACKET_HEADER_SIZE: usize = 58;
pub const FLYDRA1_PER_POINT_PAYLOAD_SIZE: usize = 58;
pub const CBOR_MAGIC: &[u8] = b"\xd9\xd9\xf7"; // D9D9F7

fn to_pascal_string(s: &str, buflen: usize) -> Result<Vec<u8>> {
    let bytes = s.as_bytes();

    if bytes.len() + 1 > buflen {
        return Err(FlydraTypesError::InputTooLong);
    }
    if bytes.len() > 255 {
        return Err(FlydraTypesError::LongStringNotImplemented);
    }
    let sz = bytes.len() as u8;
    let mut result = vec![0; buflen];
    result[0] = sz;
    (&mut result[1..(1 + bytes.len())]).copy_from_slice(bytes);
    Ok(result)
}

pub trait ReadFlydraExt: std::io::Read {
    fn read_header(&mut self) -> Result<FlydraRawUdpPacketHeader> {
        use byteorder::ReadBytesExt;

        let cam_name = self.read_pascal_string(CAM_NAME_LEN)?;
        let timestamp = self.read_f64::<LittleEndian>()?;
        let timestamp = match timestamp.is_nan() {
            true => None,
            false => Some(FlydraFloatTimestampLocal::<Triggerbox>::from_f64(timestamp)),
        };
        let cam_received_time = self.read_f64::<LittleEndian>()?;
        let cam_received_time = FlydraFloatTimestampLocal::<HostClock>::from_f64(cam_received_time);
        let framenumber = self.read_i32::<LittleEndian>()?;
        let len_points = self.read_i32::<LittleEndian>()?;
        assert!(len_points >= 0);
        let len_points = len_points as usize;
        let n_frames_skipped = self.read_u32::<LittleEndian>()?;

        Ok(FlydraRawUdpPacketHeader {
            cam_name,
            timestamp,
            cam_received_time,
            framenumber,
            n_frames_skipped,
            done_camnode_processing: 0.0,
            preprocess_stamp: 0.0,
            image_processing_steps: ImageProcessingSteps::empty(),
            len_points,
        })
    }

    fn read_points(&mut self, len_points: usize) -> Result<Vec<FlydraRawUdpPoint>> {
        (0..len_points).map(|_i| self.read_flydra_point()).collect()
    }

    fn read_flydra_packet(&mut self) -> Result<FlydraRawUdpPacket> {
        let header = self.read_header()?;
        let points = self.read_points(header.len_points as usize)?;
        Ok(FlydraRawUdpPacket::from_header_and_points(header, points))
    }
    fn read_flydra_point(&mut self) -> Result<FlydraRawUdpPoint> {
        use byteorder::ReadBytesExt;

        let x0_abs = self.read_f64::<LittleEndian>()?;
        let y0_abs = self.read_f64::<LittleEndian>()?;
        let area = self.read_f64::<LittleEndian>()?;
        let slope = self.read_f64::<LittleEndian>()?;
        let eccentricity = self.read_f64::<LittleEndian>()?;
        let slope_found = self.read_u8()?;
        let cur_val = self.read_u8()?;
        let mean_val = self.read_f64::<LittleEndian>()?;
        let sumsqf_val = self.read_f64::<LittleEndian>()?;

        let maybe_slope_eccentricty = if slope_found == 0 {
            None
        } else {
            Some((slope, eccentricity))
        };

        Ok(FlydraRawUdpPoint {
            x0_abs,
            y0_abs,
            area,
            maybe_slope_eccentricty,
            cur_val,
            mean_val,
            sumsqf_val,
        })
    }
    fn read_pascal_string(&mut self, buflen: usize) -> Result<String> {
        let mut buf = vec![0; buflen];

        {
            let buf_start = &mut buf[..3];
            self.read_exact(buf_start)?;

            if buf_start == CBOR_MAGIC {
                return Err(FlydraTypesError::CborDataError);
            }
        }

        {
            let buf_end = &mut buf[3..];
            self.read_exact(buf_end)?;
        }

        let sz = buf[0] as usize;
        let bytes = &buf.as_slice()[1..(1 + sz)];
        Ok(std::str::from_utf8(bytes).map(|s| s.to_string())?)
    }
}

/// All types that implement `Read` get methods defined in `ReadFlydraExt`
/// for free.
impl<R: std::io::Read + ?Sized> ReadFlydraExt for R {}

pub fn serialize_packet(packet: &FlydraRawUdpPacket, hack_binning: Option<u8>) -> Result<Vec<u8>> {
    // cam_id_count = 30
    // recv_pt_header_fmt = '<%dpddliI'%(cam_id_count,)
    // data = struct.pack(flydra.common_variables.recv_pt_header_fmt,
    //                     self.cam_id,
    //                     timestamp,cam_received_time,
    //                     framenumber,len(points),n_frames_skipped)
    let mut result = to_pascal_string(&packet.cam_name, CAM_NAME_LEN)?;
    let write_ts = match packet.timestamp {
        Some(ref ts) => ts.as_f64(),
        None => std::f64::NAN,
    };
    result.write_f64::<LittleEndian>(write_ts)?;
    result.write_f64::<LittleEndian>(packet.cam_received_time.as_f64())?;
    result.write_i32::<LittleEndian>(packet.framenumber)?;
    let len_points = packet.points.len() as i32;
    result.write_i32::<LittleEndian>(len_points)?;

    result.write_u32::<LittleEndian>(packet.n_frames_skipped)?;

    for pt in packet.points.iter() {
        let buf = serialize_point(pt, hack_binning)?;
        result.extend(&buf);
    }

    Ok(result)
}

pub fn deserialize_packet(buf: &[u8]) -> Result<FlydraRawUdpPacket> {
    std::io::BufReader::new(buf).read_flydra_packet()
}

pub fn serialize_point(pt: &FlydraRawUdpPoint, hack_binning: Option<u8>) -> Result<Vec<u8>> {
    // recv_pt_fmt = '<dddddBBdd'
    // pt = (x0_abs, y0_abs, area, slope, eccentricity,
    //   slope_found, cur_val, mean_val, sumsqf_val)
    let (slope, eccentricity, slope_found) = match pt.maybe_slope_eccentricty {
        Some((s, e)) => (s, e, 1),
        None => (std::f64::NAN, std::f64::NAN, 0),
    };

    let xscale;
    let yscale;
    let area;

    if let Some(bins) = hack_binning {
        xscale = bins as f64;
        yscale = bins as f64;
        area = match bins {
            1 => pt.area,
            2 => pt.area * pt.area,
            _ => {
                unimplemented!();
            }
        };
    } else {
        xscale = 1.0;
        yscale = 1.0;
        area = pt.area;
    }

    let mut result = Vec::with_capacity(450); // 450 = 7*64+2
    result.write_f64::<LittleEndian>(pt.x0_abs * xscale)?;
    result.write_f64::<LittleEndian>(pt.y0_abs * yscale)?;
    result.write_f64::<LittleEndian>(area)?;
    result.write_f64::<LittleEndian>(slope)?;
    result.write_f64::<LittleEndian>(eccentricity)?;
    result.write_u8(slope_found)?;
    result.write_u8(pt.cur_val)?;
    result.write_f64::<LittleEndian>(pt.mean_val as f64)?;
    result.write_f64::<LittleEndian>(pt.sumsqf_val as f64)?;
    Ok(result)
}

pub fn deserialize_point(buf: &[u8]) -> Result<FlydraRawUdpPoint> {
    std::io::BufReader::new(buf).read_flydra_point()
}
