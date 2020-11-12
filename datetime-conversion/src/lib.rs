extern crate chrono;

use chrono::{DateTime, Local, TimeZone};

pub fn datetime_to_f64<TZ>(dt: &DateTime<TZ>) -> f64
    where
        TZ: TimeZone,
{
    let secs = dt.timestamp() as i32;
    let nsecs = dt.timestamp_subsec_nanos() as i32;
    (secs as f64) + (nsecs as f64 * 1e-9)
}

pub fn f64_to_datetime(timestamp_f64: f64) -> DateTime<Local> {
    let secs_f = timestamp_f64.floor();
    let secs = secs_f as i64;
    let nsecs = ((timestamp_f64 - secs_f) * 1e9) as u32;
    Local.timestamp(secs, nsecs)
}
