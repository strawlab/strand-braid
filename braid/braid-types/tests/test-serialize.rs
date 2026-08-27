// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use braid_types::{
    FlydraFloatTimestampLocal, FlydraRawUdpPacket, FlydraRawUdpPoint, HostClock,
    TriggerClockInfoRow, Triggerbox,
};

fn make_test_packet() -> FlydraRawUdpPacket {
    let cam_name = "cam_id".to_string();
    let timestamp = 12.34;
    let timestamp = Some(FlydraFloatTimestampLocal::<Triggerbox>::from_f64(timestamp));
    let cam_received_time = FlydraFloatTimestampLocal::<HostClock>::from_f64(123.456);
    let device_timestamp = Some(123456);
    let block_id = Some(987654);
    let framenumber = 42;

    let points: Vec<FlydraRawUdpPoint> = vec![];

    FlydraRawUdpPacket {
        cam_name,
        timestamp,
        cam_received_time,
        device_timestamp,
        block_id,
        framenumber,
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

    let mut encoded = Vec::new();
    {
        use serde::ser::Serialize;
        let mut serializer = serde_cbor::ser::Serializer::new(&mut encoded);
        serializer.self_describe().unwrap();
        pt_orig.serialize(&mut serializer).unwrap();
    }

    // decode it.
    let pt_new: FlydraRawUdpPoint = serde_cbor::from_slice(&encoded).unwrap();
    assert_eq!(pt_new, pt_orig);
}

#[test]
fn test_cbor_packet() {
    let packet_orig = make_test_packet();

    let mut encoded = Vec::new();
    {
        use serde::ser::Serialize;
        let mut serializer = serde_cbor::ser::Serializer::new(&mut encoded);
        serializer.self_describe().unwrap();
        packet_orig.serialize(&mut serializer).unwrap();
    }

    // decode it.
    let packet_new: FlydraRawUdpPacket = serde_cbor::from_slice(&encoded).unwrap();
    assert_eq!(packet_new, packet_orig);
}

#[test]
fn test_serialize_timestamps_to_csv() -> eyre::Result<()> {
    use chrono::TimeZone;

    let t1_orig = 123.123456789;
    let t2_orig = FlydraFloatTimestampLocal::<HostClock>::from(
        chrono::Utc.with_ymd_and_hms(2100, 1, 1, 0, 1, 1).unwrap(),
    )
    .as_f64();
    let row_orig = TriggerClockInfoRow {
        start_timestamp: strand_datetime_conversion::f64_to_datetime(t1_orig).into(),
        framecount: 123,
        tcnt: 45,
        stop_timestamp: strand_datetime_conversion::f64_to_datetime(t2_orig).into(),
    };

    let mut wtr = csv::Writer::from_writer(vec![]);
    wtr.serialize(&row_orig)?;
    let buf = wtr.into_inner()?;

    let mut rdr = csv::Reader::from_reader(buf.as_slice());
    let mut iter = rdr.deserialize();

    let row_found: TriggerClockInfoRow = iter.next().unwrap()?;
    assert_eq!(row_orig, row_found);

    let t1_found = row_found.start_timestamp.as_f64();
    assert_eq!(t1_orig, t1_found);
    let t2_found = row_found.stop_timestamp.as_f64();
    assert_eq!(t2_orig, t2_found);

    Ok(())
}

/// A camera built before `feature_detect_settings` existed does not send it.
/// Braid must still accept its registration, reading the field as `None`.
#[test]
fn register_new_camera_without_feature_detect_settings() {
    use braid_types::{PngImageData, RawCamName, RegisterNewCamera};

    let older_camera = serde_json::json!({
        "raw_cam_name": "Basler-1234",
        "http_camserver_info": null,
        "cam_settings_data": null,
        "current_image_png": {"data": []},
        "camera_periodic_signal_period_usec": null,
    });

    let msg: RegisterNewCamera = serde_json::from_value(older_camera).unwrap();
    assert_eq!(msg.raw_cam_name, RawCamName::new("Basler-1234".to_string()));
    assert!(msg.feature_detect_settings.is_none());

    // And the current form round-trips.
    let current = RegisterNewCamera {
        raw_cam_name: RawCamName::new("Basler-1234".to_string()),
        http_camserver_info: None,
        cam_settings_data: None,
        feature_detect_settings: Some(braid_types::UpdateFeatureDetectSettings {
            current_feature_detect_settings: Default::default(),
        }),
        current_image_png: PngImageData::from(Vec::new()),
        camera_periodic_signal_period_usec: None,
    };
    let buf = serde_json::to_string(&current).unwrap();
    assert_eq!(current, serde_json::from_str(&buf).unwrap());
}
