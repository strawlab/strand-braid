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
        let value_f64 = datetime_conversion::datetime_to_f64(&dt);
        let value_f64 = value_f64.into();
        let source = std::marker::PhantomData;
        Self { value_f64, source }
    }

    pub fn from_f64(value_f64: f64) -> Self {
        if value_f64.is_nan() {
            panic!("cannot convert NaN to FlydraFloatTimestampLocal");
        }
        Self::from_notnan_f64(value_f64.into())
    }

    pub fn from_notnan_f64(value_f64: NotNan<f64>) -> Self {
        let source = std::marker::PhantomData;
        Self { value_f64, source }
    }

    pub fn as_f64(&self) -> f64 {
        self.value_f64.into()
    }
}

#[test]
#[should_panic]
fn test_nan_handling() {
    let _ts = FlydraFloatTimestampLocal::<Triggerbox>::from_f64(std::f64::NAN);
}
