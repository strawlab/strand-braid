//! Convert between [chrono::DateTime] and f64 representation as used in [Strand
//! Camera](https://strawlab.org/strand-cam) and
//! [Braid](https://strawlab.org/braid).

// Copyright 2020-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use chrono::{DateTime, TimeZone, Utc};

/// Converts a [chrono::DateTime] to an f64 timestamp representation.
///
/// This function converts a datetime with any timezone to a floating-point
/// timestamp where the integer part represents seconds since Unix epoch
/// and the fractional part represents nanoseconds as a decimal fraction.
///
/// # Arguments
///
/// * `dt` - A reference to a [DateTime] with any timezone
///
/// # Returns
///
/// Returns an f64 timestamp where:
/// - Integer part: seconds since Unix epoch (1970-01-01 00:00:00 UTC)
/// - Fractional part: nanoseconds expressed as a decimal (e.g., 0.123456789)
///
/// # Example
///
/// ```rust
/// use chrono::{DateTime, Utc, TimeZone, Timelike};
/// use strand_datetime_conversion::datetime_to_f64;
///
/// let dt = Utc.with_ymd_and_hms(2023, 12, 25, 15, 30, 45).unwrap()
///     .with_nanosecond(123456789).unwrap();
/// let timestamp = datetime_to_f64(&dt);
/// // timestamp will be something like 1703518245.123456789
/// ```
pub fn datetime_to_f64<TZ>(dt: &DateTime<TZ>) -> f64
where
    TZ: TimeZone,
{
    let secs = dt.timestamp() as i32;
    let nsecs = dt.timestamp_subsec_nanos() as i32;
    (secs as f64) + (nsecs as f64 * 1e-9)
}

/// Converts an f64 timestamp to a [chrono::DateTime] in UTC timezone.
///
/// This is a convenience function that converts a floating-point timestamp
/// to a UTC datetime. For timezone-specific conversion, use [f64_to_datetime_any].
///
/// # Arguments
///
/// * `timestamp_f64` - A floating-point timestamp where the integer part
///   represents seconds since Unix epoch and the fractional part represents
///   nanoseconds as a decimal fraction
///
/// # Returns
///
/// Returns a [DateTime] representing the timestamp in UTC timezone.
///
/// # Panics
///
/// Panics if the timestamp is invalid or out of range for the chrono library.
///
/// # Example
///
/// ```rust
/// use strand_datetime_conversion::f64_to_datetime;
///
/// let timestamp = 1703518245.123456789;
/// let dt = f64_to_datetime(timestamp);
/// // dt will be 2023-12-25 15:30:45.123456789 UTC
/// ```
pub fn f64_to_datetime(timestamp_f64: f64) -> DateTime<Utc> {
    f64_to_datetime_any(timestamp_f64, Utc)
}

/// Converts an f64 timestamp to a [chrono::DateTime] in the specified timezone.
///
/// This function provides full control over the target timezone for the
/// converted datetime. The input timestamp is interpreted as seconds since
/// Unix epoch (always in UTC), but the resulting DateTime will be in the
/// specified timezone.
///
/// # Arguments
///
/// * `timestamp_f64` - A floating-point timestamp where the integer part
///   represents seconds since Unix epoch and the fractional part represents
///   nanoseconds as a decimal fraction
/// * `tz` - The target timezone for the resulting DateTime
///
/// # Returns
///
/// Returns a [DateTime] representing the timestamp in the specified timezone.
///
/// # Panics
///
/// Panics if the timestamp is invalid or out of range for the chrono library.
///
/// # Example
///
/// ```rust
/// use chrono::Utc;
/// use strand_datetime_conversion::f64_to_datetime_any;
///
/// let timestamp = 1703518245.123456789;
/// let dt_utc = f64_to_datetime_any(timestamp, Utc);
/// // dt_utc will be 2023-12-25 15:30:45.123456789 UTC
/// ```
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
