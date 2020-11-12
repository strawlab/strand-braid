//! get the IANA time zone for the current system
//!
//! ```
//! extern crate get_timezone;
//! println!("current: {}", get_timezone::get_timezone().unwrap());
//! ```

#[cfg(target_os = "macos")]
extern crate core_foundation;

/// Error types
#[derive(Debug)]
pub enum GetTimezoneError {
    /// Failed to parse
    FailedParsingString,
    /// Unknown time zone
    UnknownTimeZone,
    /// Wrapped IO error
    IoError(std::io::Error),
}

impl std::error::Error for GetTimezoneError {}

impl std::fmt::Display for GetTimezoneError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        use GetTimezoneError::*;
        let descr = match self {
            &FailedParsingString => "GetTimezoneError::FailedParsingString",
            &UnknownTimeZone => "GetTimezoneError::UnknownTimeZone",
            &IoError(_) => "GetTimezoneError::IoError(_)",
        };

        write!(f, "{}", descr)
    }
}

impl std::convert::From<std::io::Error> for GetTimezoneError {
    fn from(e: std::io::Error) -> GetTimezoneError {
        GetTimezoneError::IoError(e)
    }
}

#[cfg(target_os = "windows")]
use chrono_tz::{Asia, Australia, Etc, Europe, Pacific, US};

// This is a hacky list I put together. Better might be to parse, e.g.
// https://github.com/unicode-org/cldr/blob/master/common/supplemental/metaZones.xml
// specifically the `territory="001"` values.
//
// Even better would be to get the territory name (how exactly with the windows
// api?) and look up the zone directly.
//
// The ordering of this list is important - the first matched offset will be the
// value returned.
#[cfg(target_os = "windows")]
const ORDERED_LIST: &[chrono_tz::Tz] = &[
    Europe::Berlin,
    Europe::London,
    Europe::Dublin,
    Europe::Moscow,
    Europe::Samara,
    Europe::Istanbul,
    Europe::Volgograd,
    Europe::Bucharest,
    Europe::Minsk,
    US::Pacific,
    US::Mountain,
    US::Central,
    US::Eastern,
    Asia::Baku,
    Asia::Dhaka,
    Asia::Thimphu,
    Asia::Kuching,
    Asia::Brunei,
    Asia::Shanghai,
    Asia::Dhaka,
    Asia::Tbilisi,
    Asia::Dubai,
    Asia::Hong_Kong,
    Asia::Calcutta,
    Asia::Bangkok,
    Asia::Jakarta,
    Asia::Tehran,
    Asia::Jerusalem,
    Asia::Tokyo,
    Asia::Seoul,
    Asia::Colombo,
    Asia::Novosibirsk,
    Asia::Katmandu,
    Asia::Manila,
    Asia::Singapore,
    Australia::Adelaide,
    Australia::Eucla,
    Australia::Sydney,
    Australia::Perth,
    Australia::Lord_Howe,
    Pacific::Auckland,
    Etc::GMT,
    Etc::GMT0,
    Etc::GMTMinus0,
    Etc::GMTMinus1,
    Etc::GMTMinus2,
    Etc::GMTMinus3,
    Etc::GMTMinus4,
    Etc::GMTMinus5,
    Etc::GMTMinus6,
    Etc::GMTMinus7,
    Etc::GMTMinus8,
    Etc::GMTMinus9,
    Etc::GMTMinus10,
    Etc::GMTMinus11,
    Etc::GMTMinus12,
    Etc::GMTMinus13,
    Etc::GMTMinus14,
    Etc::GMTPlus0,
    Etc::GMTPlus1,
    Etc::GMTPlus2,
    Etc::GMTPlus3,
    Etc::GMTPlus4,
    Etc::GMTPlus5,
    Etc::GMTPlus6,
    Etc::GMTPlus7,
    Etc::GMTPlus8,
    Etc::GMTPlus9,
    Etc::GMTPlus10,
    Etc::GMTPlus11,
    Etc::GMTPlus12,
    Etc::Greenwich,
    Etc::UCT,
    Etc::UTC,
    Etc::Universal,
    Etc::Zulu,
];

#[cfg(target_os = "windows")]
#[inline]
fn target_os_specific_get_timezone() -> Result<String, GetTimezoneError> {
    let sys_time = winapi::um::minwinbase::SYSTEMTIME {
        wYear: 0,
        wMonth: 0,
        wDayOfWeek: 0,
        wDay: 0,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    };
    let mut time_zone = winapi::um::timezoneapi::TIME_ZONE_INFORMATION {
        Bias: 0,
        StandardName: [0; 32],
        StandardDate: sys_time,
        StandardBias: 0,
        DaylightName: [0; 32],
        DaylightDate: sys_time,
        DaylightBias: 0,
    };
    let res: winapi::shared::minwindef::DWORD;
    unsafe {
        res = winapi::um::timezoneapi::GetTimeZoneInformation(&mut time_zone);
    }

    // See https://docs.microsoft.com/en-us/windows/win32/api/timezoneapi/nf-timezoneapi-gettimezoneinformation
    let current_windows_bias_minutes = match res {
        // winapi::um::timezoneapi::TIME_ZONE_ID_INVALID => {}
        winapi::um::winnt::TIME_ZONE_ID_UNKNOWN => time_zone.Bias,
        winapi::um::winnt::TIME_ZONE_ID_STANDARD => time_zone.Bias + time_zone.StandardBias,
        winapi::um::winnt::TIME_ZONE_ID_DAYLIGHT => time_zone.Bias + time_zone.DaylightBias,
        winapi::um::timezoneapi::TIME_ZONE_ID_INVALID => {
            panic!("invalid time zone id");
        }
        _ => {
            panic!("unknown return value");
        }
    };

    let now_utc_naive = chrono::Utc::now().naive_utc();
    for test_tz in ORDERED_LIST {
        use chrono::offset::{Offset, TimeZone};
        let this_offset = test_tz.offset_from_utc_datetime(&now_utc_naive);
        let fix = this_offset.fix();
        let this_offset_seconds = fix.utc_minus_local();
        if this_offset_seconds == (current_windows_bias_minutes * 60) {
            return Ok(test_tz.name().to_string());
        }
    }

    Err(GetTimezoneError::UnknownTimeZone)
}

#[cfg(target_os = "linux")]
#[inline]
fn target_os_specific_get_timezone() -> Result<String, GetTimezoneError> {
    // see https://stackoverflow.com/a/12523283
    use std::io::Read;

    let fname = "/etc/timezone";
    let mut f = std::fs::File::open(&fname)?;
    let mut contents = String::new();
    f.read_to_string(&mut contents)?;
    Ok(contents.trim().to_string())
}

#[cfg(target_os = "macos")]
#[inline]
fn target_os_specific_get_timezone() -> Result<String, GetTimezoneError> {
    let tz = core_foundation::timezone::CFTimeZone::system();

    // Get string like ""Europe/Berlin (GMT+2) offset 7200 (Daylight)""
    let mut str1 = format!("{:?}", tz);

    // strip leading double quotes
    while str1.starts_with('"') {
        str1 = str1[1..].to_string();
    }

    match str1.split_whitespace().next() {
        Some(s) => Ok(s.to_string()),
        None => Err(GetTimezoneError::FailedParsingString),
    }
}

/// Returns IANA timezone string for the current system (e.g. `"Europe/Berlin"`)
pub fn get_timezone() -> Result<String, GetTimezoneError> {
    target_os_specific_get_timezone() // compile time error on unknown targets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_current() {
        println!("current: {}", get_timezone().unwrap());
    }
}
