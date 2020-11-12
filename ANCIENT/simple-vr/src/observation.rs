use super::time;

#[derive(Debug, RustcDecodable, RustcEncodable)]
pub struct Observation {
    timestamp: Timestamp,
    feature: Option<ObservedFeature>,
}

impl Observation {
    pub fn new(timestamp: Timestamp, feature: Option<ObservedFeature>) -> Observation {
        Observation {
            timestamp: timestamp,
            feature: feature,
        }
    }
    #[inline]
    pub fn timestamp(&self) -> &Timestamp {
        &self.timestamp
    }

    #[inline]
    pub fn feature(&self) -> &Option<ObservedFeature> {
        &self.feature
    }
}

#[derive(Debug, RustcDecodable, RustcEncodable)]
pub struct Timestamp {
    sec: i64,
    nsec: i32,
}
impl Timestamp {
    pub fn new_from_now() -> Timestamp {
        let timespec = time::get_time();
        Timestamp {
            sec: timespec.sec,
            nsec: timespec.nsec,
        }
    }
    pub fn as_timespec(&self) -> time::Timespec {
        time::Timespec::new(self.sec, self.nsec)
    }
}

#[derive(Debug, RustcDecodable, RustcEncodable)]
pub struct ObservedFeature {
    pixel_xy: [f32; 2],
    theta: Option<f32>,
}

impl ObservedFeature {
    pub fn new(x: f32, y: f32, theta: Option<f32>) -> ObservedFeature {
        ObservedFeature {
            pixel_xy: [x, y],
            theta: theta,
        }
    }

    #[inline]
    pub fn x(&self) -> f32 {
        self.pixel_xy[0]
    }

    #[inline]
    pub fn y(&self) -> f32 {
        self.pixel_xy[1]
    }
}
