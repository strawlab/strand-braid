extern crate chrono;
extern crate datetime_conversion;

use chrono::TimeZone;
use datetime_conversion::{datetime_to_f64, f64_to_datetime};

#[test]
fn test_roundtrip_local() {
    let now = chrono::Local::now();
    println!("now {:?}", now);
    let now_f64 = datetime_to_f64(&now);
    println!("now_f64 {:?}", now_f64);
    let dt = f64_to_datetime(now_f64);
    println!("dt {:?}", dt);
    let dt_f64 = datetime_to_f64(&dt);

    let diff = now_f64 - dt_f64;

    let epsilon = 1e-6;
    assert!(diff.abs() < epsilon);
}

#[test]
fn test_roundtrip_nonlocal() {

    let now = chrono::FixedOffset::east(5 * 60 * 60)
        .ymd(2016, 11, 08)
        .and_hms(0, 0, 0);
    println!("now {:?}", now);
    let now_f64 = datetime_to_f64(&now);
    println!("now_f64 {:?}", now_f64);
    let dt = f64_to_datetime(now_f64);
    println!("dt {:?}", dt);
    let dt_f64 = datetime_to_f64(&dt);

    let diff = now_f64 - dt_f64;

    let epsilon = 1e-6;
    assert!(diff.abs() < epsilon);
}
