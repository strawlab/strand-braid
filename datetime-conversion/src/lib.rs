extern crate chrono;

use chrono::{DateTime, TimeZone, Utc};

pub fn datetime_to_f64<TZ>(dt: &DateTime<TZ>) -> f64
where
    TZ: TimeZone,
{
    let secs = dt.timestamp() as i32;
    let nsecs = dt.timestamp_subsec_nanos() as i32;
    (secs as f64) + (nsecs as f64 * 1e-9)
}

pub fn f64_to_datetime(timestamp_f64: f64) -> DateTime<Utc> {
    f64_to_datetime_any(timestamp_f64, Utc)
}

pub fn f64_to_datetime_any<TZ>(timestamp_f64: f64, tz: TZ) -> DateTime<TZ>
where
    TZ: chrono::TimeZone,
{
    let secs_f = timestamp_f64.floor();
    let secs = secs_f as i64;
    let nsecs = ((timestamp_f64 - secs_f) * 1e9) as u32;
    tz.timestamp_opt(secs, nsecs).unwrap()
}

#[test]
fn test_roundtrip() {
    for orig in &[0.0, 123.456, 456.789, 1634378218.4130154] {
        let rt = datetime_to_f64(&f64_to_datetime(*orig));
        dbg!(orig);
        dbg!(rt);
        assert!((orig - rt).abs() < 1e-9);
    }
}

#[test]
fn test_precision() {
    use chrono::TimeZone;

    let t1_orig = 123.123456789;
    let t2_orig = datetime_to_f64(&chrono::Utc.with_ymd_and_hms(2100, 1, 1, 0, 1, 1).unwrap());

    // Ensure microsecond precision is kept in floating point representations.
    let t1_bad = t1_orig + 1e-6;
    assert!(t1_orig != t1_bad);

    let t2_bad = t2_orig + 1e-6;
    assert!(t2_orig != t2_bad);
}
