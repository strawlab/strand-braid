use cgmath::{Vector3, Matrix3};

use super::time;
use super::config;
use super::observation::{Observation, ObservedFeature};

#[derive(Clone,Debug)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

pub struct Tracker {
    last_point: Option<Point>,
    // cfg: config::TrackerConfig,
    mat3: Matrix3<f32>,
}

impl Tracker {
    pub fn new(cfg: &config::TrackerConfig) -> Tracker {
        let mat3 = Into::<Matrix3<f32>>::into(cfg.pixels_to_meters_matrix3);

        Tracker {
            last_point: None,
            // cfg: cfg.clone(), // TODO eventually just keep a reference
            mat3: mat3,
        }
    }

    pub fn handle_new_observation(&mut self, features: &Observation) {
        let stamp = features.timestamp().as_timespec();
        let now = time::get_time();
        let age = now - stamp;
        if age.num_milliseconds() < 0 {
            error!("ignoring data from the future");
            return;
        }

        if age.num_milliseconds() > 5000 {
            error!("ignoring data more than 5 seconds old.");
            return;
        }

        if let &Some(ref f) = features.feature() {
            self.last_point = Some(self.to_meters(&f));
        }
    }

    fn to_meters(&self, pixels: &ObservedFeature) -> Point {
        let input = Vector3::new(pixels.x(), pixels.y(), 1.0); // make homogeneous coord
        let output = self.mat3 * input;
        Point {
            x: output.x / output.z,
            y: output.y / output.z,
        }
    }


    pub fn get_state(&mut self) -> Option<Point> {
        self.last_point.clone()
    }
}
