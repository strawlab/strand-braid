use super::FirehoseImageData;

// TODO: put inner behind std::rc::Rc to prevent cloning of data

#[derive(Clone, PartialEq)]
pub struct VideoData {
    pub inner: Option<FirehoseImageData>,
}

impl Default for VideoData {
    fn default() -> Self {
        Self { inner: None }
    }
}
