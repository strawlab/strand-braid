#[macro_use]
extern crate serde_derive;
use std::collections::HashSet;
extern crate http_video_streaming_types;

use http_video_streaming_types::FirehoseCallbackInner;

#[derive(Clone,PartialEq,Serialize,Deserialize)]
pub struct ImageInfo {
    pub image_width: u32,
    pub image_height: u32,
    pub measured_fps: f32,
}

#[derive(Clone,PartialEq,Serialize,Deserialize)]
pub struct StoreType {
    pub image_names: HashSet<String>,
    pub image_info: Option<ImageInfo>,
}

#[derive(Serialize,Deserialize,Clone)]
pub enum RtImageViewerCallback {
    FirehoseNotify(FirehoseCallbackInner),
}

pub const RT_IMAGE_EVENTS_URL_PATH: &'static str = "rt-image-events";
pub const RT_IMAGE_EVENT_NAME: &'static str = "rt-image";
