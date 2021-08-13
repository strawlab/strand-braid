use chrono::{DateTime, TimeZone};

pub(crate) fn datetime_to_f64<TZ>(dt: &DateTime<TZ>) -> f64
where
    TZ: TimeZone,
{
    let secs = dt.timestamp() as i32;
    let nsecs = dt.timestamp_subsec_nanos() as i32;
    (secs as f64) + (nsecs as f64 * 1e-9)
}
