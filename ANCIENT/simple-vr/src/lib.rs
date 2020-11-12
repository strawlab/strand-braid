#[macro_use]
extern crate log;
extern crate env_logger;
extern crate eventual;
extern crate time;
extern crate rustc_serialize;
extern crate fastimage as ipp;
extern crate machine_vision_formats;
extern crate cgmath;

#[macro_use]
extern crate glium;

#[macro_use]
extern crate lazy_static;

// extern crate reactive_cam;
// #[cfg(feature = "camiface")]
// extern crate reactive_camiface;

// #[cfg(feature = "flycap")]
// extern crate reactive_cam_flycap;

pub mod vr_display;
pub mod cam_view;

pub mod config;
pub mod obj_load;
pub mod observation;
pub mod camera;
pub mod tracker;
pub mod image_processing;
pub mod network_input;
