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
    let now = chrono::FixedOffset::east_opt(5 * 60 * 60)
        .unwrap()
        .with_ymd_and_hms(2016, 11, 08, 0, 0, 0)
        .unwrap();
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
