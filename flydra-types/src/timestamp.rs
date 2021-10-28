use crate::*;

pub trait Source {}

#[derive(Debug, Clone, PartialEq)]
pub struct Triggerbox;
impl Source for Triggerbox {}

#[derive(Debug, Clone, PartialEq)]
pub struct HostClock;
impl Source for HostClock {}

/// A type that represents a timestamp but is serialized to an f64.
#[derive(Debug, Clone, PartialEq)]
pub struct FlydraFloatTimestampLocal<S> {
    value_f64: NotNan<f64>,
    source: std::marker::PhantomData<S>,
}

impl<S: Source, TZ: chrono::TimeZone> From<&chrono::DateTime<TZ>> for FlydraFloatTimestampLocal<S> {
    fn from(orig: &chrono::DateTime<TZ>) -> Self {
        FlydraFloatTimestampLocal::from_dt(orig)
    }
}

impl<S: Source, TZ: chrono::TimeZone> From<chrono::DateTime<TZ>> for FlydraFloatTimestampLocal<S> {
    fn from(val: chrono::DateTime<TZ>) -> FlydraFloatTimestampLocal<S> {
        FlydraFloatTimestampLocal::from_dt(&val)
    }
}

impl<'a, S: Source> From<&'a FlydraFloatTimestampLocal<S>> for chrono::DateTime<chrono::Local> {
    fn from(orig: &'a FlydraFloatTimestampLocal<S>) -> chrono::DateTime<chrono::Local> {
        datetime_conversion::f64_to_datetime(orig.value_f64.into_inner())
    }
}

impl<S: Source> From<FlydraFloatTimestampLocal<S>> for chrono::DateTime<chrono::Local> {
    fn from(orig: FlydraFloatTimestampLocal<S>) -> chrono::DateTime<chrono::Local> {
        datetime_conversion::f64_to_datetime(orig.value_f64.into_inner())
    }
}

assert_impl_all!(val; FlydraFloatTimestampLocal<Triggerbox>, PartialEq);

impl<S: Source> FlydraFloatTimestampLocal<S> {
    pub fn from_dt<TZ: chrono::TimeZone>(dt: &chrono::DateTime<TZ>) -> Self {
        let value_f64 = datetime_conversion::datetime_to_f64(dt);
        let value_f64 = value_f64.into();
        let source = std::marker::PhantomData;
        Self { value_f64, source }
    }

    pub fn from_f64(value_f64: f64) -> Self {
        assert!(
            !value_f64.is_nan(),
            "cannot convert NaN to FlydraFloatTimestampLocal"
        );
        Self::from_notnan_f64(value_f64.into())
    }

    pub fn from_notnan_f64(value_f64: NotNan<f64>) -> Self {
        let source = std::marker::PhantomData;
        Self { value_f64, source }
    }

    #[inline(always)]
    pub fn as_f64(&self) -> f64 {
        self.value_f64.into()
    }
}

/// Compute the trigger time for a particular frame.
///
/// Requires both a clock model (general for all cameras) and a frame offset
/// (which maps the particular frame numbers for a given camera into a
/// synchronized frame number).
#[inline]
pub fn get_start_ts(
    clock_model: Option<&ClockModel>,
    frame_offset: Option<u64>,
    frame: u64,
) -> Option<FlydraFloatTimestampLocal<Triggerbox>> {
    if let Some(frame_offset) = frame_offset {
        if let Some(cm) = clock_model {
            let ts: f64 = ((frame - frame_offset) as f64) * cm.gain + cm.offset;
            let ts = FlydraFloatTimestampLocal::<Triggerbox>::from_f64(ts);
            return Some(ts);
        }
    }
    None
}

#[test]
#[should_panic]
fn test_nan_handling() {
    let _ts = FlydraFloatTimestampLocal::<Triggerbox>::from_f64(std::f64::NAN);
}

#[test]
fn ensure_conversion() {
    use chrono::{DateTime, Utc};
    let t1 = DateTime::<Utc>::from_utc(chrono::NaiveDateTime::from_timestamp(60, 123_456_789), Utc);
    let t2 = FlydraFloatTimestampLocal::<HostClock>::from(t1);
    let t3 = t2.value_f64.into_inner();
    assert!((t3 - 60.123456789).abs() < 1e-10);
}
