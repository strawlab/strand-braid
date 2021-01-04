#![recursion_limit = "1000"]

extern crate gloo;
extern crate wasm_bindgen;
extern crate web_sys;

extern crate chrono;
extern crate http;
extern crate serde;
extern crate serde_yaml;
extern crate uuid;
extern crate yew;
extern crate yew_tincture;

extern crate bui_backend_types;
extern crate enum_iter;
extern crate http_video_streaming_types;
extern crate rust_cam_bui_types;

use http_video_streaming_types::ToClient as FirehoseImageData;

pub mod components;
pub mod services;
pub mod video_data;
