extern crate flydra_types;
extern crate serde_cbor;

use flydra_types::{FlydraRawUdpPacket, FlydraRawUdpPoint, ImageProcessingSteps};

use flydra_types::{FlydraFloatTimestampLocal, HostClock, Triggerbox};

fn make_test_packet() -> FlydraRawUdpPacket {
    let cam_name = "cam_id".to_string();
    let timestamp = 12.34;
    let timestamp = Some(FlydraFloatTimestampLocal::<Triggerbox>::from_f64(timestamp));
    let cam_received_time = FlydraFloatTimestampLocal::<HostClock>::from_f64(123.456);
    let device_timestamp = std::num::NonZeroU64::new(123456);
    let block_id = std::num::NonZeroU64::new(987654);
    let framenumber = 42;
    let n_frames_skipped = 6;

    let points: Vec<FlydraRawUdpPoint> = vec![];

    FlydraRawUdpPacket {
        cam_name,
        timestamp,
        cam_received_time,
        device_timestamp,
        block_id,
        framenumber,
        n_frames_skipped,
        done_camnode_processing: 0.0,
        preprocess_stamp: 0.0,
        image_processing_steps: ImageProcessingSteps::empty(),
        points,
    }
}

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
fn test_cbor_point() {
    let pt_orig = make_test_point();
    let encoded = serde_cbor::ser::to_vec_packed_sd(&pt_orig).unwrap();

    // decode it.
    let pt_new: FlydraRawUdpPoint = serde_cbor::from_slice(&encoded).unwrap();
    assert_eq!(pt_new, pt_orig);
}

#[test]
fn test_cbor_packet() {
    let packet_orig = make_test_packet();
    let encoded = serde_cbor::ser::to_vec_packed_sd(&packet_orig).unwrap();

    // decode it.
    let packet_new: FlydraRawUdpPacket = serde_cbor::from_slice(&encoded).unwrap();
    assert_eq!(packet_new, packet_orig);
}
