#![recursion_limit = "1000"]

extern crate http;
#[macro_use]
extern crate stdweb;
extern crate yew;
#[macro_use]
extern crate failure;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate chrono;
extern crate serde_yaml;
extern crate uuid;
extern crate yew_tincture;

extern crate bui_backend_types;
extern crate enum_iter;
extern crate http_video_streaming_types;
extern crate rust_cam_bui_types;

use http_video_streaming_types::ToClient as FirehoseImageData;

pub mod components;
pub mod services;
pub mod video_data;

use services::eventsource::ReadyState;

pub enum EventSourceAction {
    Connect,
    Disconnect,
    Lost(ReadyState),
}
