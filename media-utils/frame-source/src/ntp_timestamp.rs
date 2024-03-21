// Copyright 2021-2024 Scott Lamb <slamb@slamb.org>

/// The Unix epoch as an [`NtpTimestamp`].
pub(crate) const UNIX_EPOCH: NtpTimestamp = NtpTimestamp((2_208_988_800) << 32);

/// A wallclock time represented using the format of the Network Time Protocol.
///
/// NTP timestamps are in a fixed-point representation of seconds since
/// 0h UTC on 1 January 1900. The top 32 bits represent the integer part
/// (wrapping around every 68 years) and the bottom 32 bits represent the
/// fractional part.
///
/// This is a simple wrapper around a `u64` in that format, with a `Display`
/// impl that writes the timestamp as a human-readable string. Currently this
/// assumes the time is within 68 years of 1970; the string will be incorrect
/// after `2038-01-19T03:14:07Z`.
///
/// An `NtpTimestamp` isn't necessarily gathered from a real NTP server.
/// Reported NTP timestamps are allowed to jump backwards and/or be complete
/// nonsense.
///
/// The NTP timestamp of the Unix epoch is available via the constant [`UNIX_EPOCH`].
#[derive(Copy, Clone, PartialEq, PartialOrd, Eq, Ord)]
pub(crate) struct NtpTimestamp(pub u64);

impl std::fmt::Display for NtpTimestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let date_time: chrono::DateTime<chrono::Local> = (*self).into();
        write!(f, "{}", date_time.format("%FT%T%.3f%:z"),)
    }
}

impl std::fmt::Debug for NtpTimestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Write both the raw and display forms.
        write!(f, "{} /* {} */", self.0, self)
    }
}

fn chrono_to_ntp<TZ>(orig: chrono::DateTime<TZ>) -> Result<NtpTimestamp, std::num::TryFromIntError>
where
    TZ: chrono::TimeZone,
{
    let epoch_naive = chrono::NaiveDate::from_ymd_opt(1900, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let epoch = chrono::TimeZone::from_local_datetime(&chrono::Utc, &epoch_naive).unwrap();
    let elapsed: chrono::TimeDelta = orig.to_utc() - epoch;
    let sec_since_epoch: u32 = elapsed.num_seconds().try_into()?;
    let nanos = elapsed.subsec_nanos();
    let frac = f64::from(nanos) / 1e9;
    let frac_int = (frac * f64::from(u32::MAX)).round() as u32;
    let val = (u64::from(sec_since_epoch) << 32) + u64::from(frac_int);
    Ok(NtpTimestamp(val))
}

impl<TZ> TryFrom<chrono::DateTime<TZ>> for NtpTimestamp
where
    TZ: chrono::TimeZone,
{
    type Error = std::num::TryFromIntError;
    fn try_from(orig: chrono::DateTime<TZ>) -> Result<Self, Self::Error> {
        chrono_to_ntp(orig)
    }
}

impl<TZ> From<NtpTimestamp> for chrono::DateTime<TZ>
where
    TZ: chrono::TimeZone,
    chrono::DateTime<TZ>: From<chrono::DateTime<chrono::Utc>>,
{
    fn from(orig: NtpTimestamp) -> Self {
        let since_epoch = orig.0.wrapping_sub(UNIX_EPOCH.0);
        let sec_since_epoch = (since_epoch >> 32) as u32;
        let frac_int = (since_epoch & 0xFFFF_FFFF) as u32;
        let frac = frac_int as f64 / f64::from(u32::MAX);
        let nanos = (frac * 1e9).round() as u32;
        let timedelta: chrono::TimeDelta =
            chrono::TimeDelta::new(i64::from(sec_since_epoch), nanos).unwrap();
        let date_time = chrono::DateTime::UNIX_EPOCH + timedelta;
        date_time.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ORIG_STR: &str = "2024-02-17T21:14:34.013+01:00";

    #[test]
    fn test_ntp_roundtrip() {
        let orig: chrono::DateTime<chrono::Utc> = ORIG_STR.parse().unwrap();
        let ntp_timestamp = chrono_to_ntp(orig).unwrap();
        let display = format!("{ntp_timestamp}");
        let parsed: chrono::DateTime<chrono::Utc> = display.parse().unwrap();
        assert_eq!(orig, parsed);
    }

    #[test]
    fn test_ntp_roundtrip_raw() {
        let orig: chrono::DateTime<chrono::Utc> = ORIG_STR.parse().unwrap();
        let ntp_timestamp = chrono_to_ntp(orig).unwrap();
        let parsed: chrono::DateTime<chrono::Utc> = ntp_timestamp.into();
        assert_eq!(orig, parsed);
    }

    #[test]
    fn test_ntp_decode() {
        let orig: chrono::DateTime<chrono::Utc> = ORIG_STR.parse().unwrap();
        assert_eq!(
            chrono_to_ntp(orig).unwrap(),
            NtpTimestamp(16824201542114736079)
        );
    }

    fn assert_roundtrip_equal(n0: NtpTimestamp) {
        let dt1 = chrono::DateTime::<chrono::Utc>::from(n0);
        let n1 = NtpTimestamp::try_from(dt1).unwrap();
        let dt2: chrono::DateTime<chrono::Utc> = n1.into();
        assert_eq!(dt1, dt2);
    }

    #[test]
    fn test_now() {
        let dt0 = chrono::Utc::now();
        let n0 = NtpTimestamp::try_from(dt0).unwrap();
        assert_roundtrip_equal(n0);
    }

    #[test]
    fn test_float_rounding() {
        // This magic number was found empirically to fail when conversion to a
        // floating-point fractional second did not round correctly. The bug has
        // now been fixed but this test ensures it does not occur again.
        assert_roundtrip_equal(NtpTimestamp(16834755242908219071));
    }
}
