extern crate flydra_types;
extern crate serde_cbor;

use flydra_types::{
    deserialize_packet, deserialize_point, serialize_packet, serialize_point, FlydraRawUdpPacket,
    FlydraRawUdpPoint, FlydraTypesError, ImageProcessingSteps, FLYDRA1_PACKET_HEADER_SIZE,
    FLYDRA1_PER_POINT_PAYLOAD_SIZE,
};

use flydra_types::{FlydraFloatTimestampLocal, HostClock, Triggerbox};

/*
import struct

cam_id_count = 30
recv_pt_fmt = '<dddddBBdd'
recv_pt_header_fmt = '<%dpddliI'%(cam_id_count,)

cam_id = 'cam_id'
raw_timestamp = 12.34
camn_received_time = 123.456
raw_framenumber = 42
n_pts = 0
n_frames_skipped = 6

header = struct.pack(recv_pt_header_fmt,            cam_id, raw_timestamp, camn_received_time,
            raw_framenumber, n_pts,n_frames_skipped)


print('%r' % header)
*/
const TEST_HEADER_BUF: &[u8; 58] = b"\x06cam_id\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\
        \x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\xaeG\xe1z\x14\xae(@w\xbe\
        \x9f\x1a/\xdd^@*\x00\x00\x00\x00\x00\x00\x00\x06\x00\x00\x00";

fn make_test_packet() -> FlydraRawUdpPacket {
    let cam_name = "cam_id".to_string();
    let timestamp = 12.34;
    let timestamp = Some(FlydraFloatTimestampLocal::<Triggerbox>::from_f64(timestamp));
    let cam_received_time = FlydraFloatTimestampLocal::<HostClock>::from_f64(123.456);
    let framenumber = 42;
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

/*
import struct

recv_pt_fmt = '<dddddBBdd'
x0_abs = 12.34
y0_abs = 56.78
area = 11.1
slope = 22.2
eccentricity = 33.3
slope_found = True
cur_val = 13
mean_val = 12345.0
sumsqf_val = 55.5
pt = (x0_abs, y0_abs, area, slope, eccentricity,
    slope_found, cur_val, mean_val, sumsqf_val)
ptbuf = struct.pack(recv_pt_fmt, *pt)
print('%r' % ptbuf)
*/
const TEST_POINT_BUF: &[u8; 58] = b"\xaeG\xe1z\x14\xae(@\xa4p=\n\xd7cL@333333&@3333336@fffff\
        \xa6@@\x01\r\x00\x00\x00\x00\x80\x1c\xc8@\x00\x00\x00\x00\x00\xc0K@";

fn make_test_point() -> FlydraRawUdpPoint {
    FlydraRawUdpPoint {
        x0_abs: 12.34,
        y0_abs: 56.78,
        area: 11.1,
        maybe_slope_eccentricty: Some((22.2, 33.3)),
        cur_val: 13,
        mean_val: 12345.0,
        sumsqf_val: 55.5,
    }
}

#[test]
fn test_serialize_packet() {
    let expected = TEST_HEADER_BUF;

    // println!("expected raw buf len: {}", expected.len());
    // println!("expected raw buf: {:x?}", expected.to_vec());

    let dgram = make_test_packet();
    let actual = serialize_packet(&dgram, None).unwrap();
    assert_eq!(expected.to_vec(), actual.to_vec());
}

#[test]
fn test_deserialize_packet() {
    let input = TEST_HEADER_BUF;
    let expected = make_test_packet();
    let actual = deserialize_packet(input).unwrap();

    let expected_size =
        FLYDRA1_PACKET_HEADER_SIZE + FLYDRA1_PER_POINT_PAYLOAD_SIZE * actual.points.len();
    assert_eq!(expected_size, input.len());

    assert_eq!(expected, actual);
}

#[test]
fn test_serialize_point() {
    let expected = TEST_POINT_BUF;
    let pt = make_test_point();
    let actual = serialize_point(&pt, None).unwrap();
    assert_eq!(expected.to_vec(), actual.to_vec());
}

#[test]
fn test_deserialize_point() {
    assert_eq!(FLYDRA1_PER_POINT_PAYLOAD_SIZE, TEST_POINT_BUF.len());

    let input = TEST_POINT_BUF;
    let expected = make_test_point();
    let actual = deserialize_point(input).unwrap();
    assert_eq!(expected, actual);
}

#[test]
fn test_cbor() {
    let pt_orig = make_test_point();
    let encoded = serde_cbor::ser::to_vec_packed_sd(&pt_orig).unwrap();

    // We expect this to fail because we are not passing our custon
    // type but rather a CBOR packet.
    let x1 = deserialize_packet(&encoded);
    match x1 {
        Ok(_) => panic!("succeeded where failure expected"),
        Err(FlydraTypesError::CborDataError) => {}
        Err(_) => panic!("incorrect failure"),
    };

    // So now we decode it as cbor.
    let pt_new: FlydraRawUdpPoint = serde_cbor::from_slice(&encoded).unwrap();
    assert_eq!(pt_new, pt_orig);
}
