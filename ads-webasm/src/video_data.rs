use super::FirehoseImageData;

#[derive(Clone, PartialEq)]
pub struct VideoData {
    inner: Option<FirehoseImageData>,
}

impl VideoData {
    pub fn new(data: FirehoseImageData) -> Self {
        Self { inner: Some(data) }
    }

    pub fn inner(&self) -> Option<&FirehoseImageData> {
        self.inner.as_ref()
    }
}

impl Default for VideoData {
    fn default() -> Self {
        Self { inner: None }
    }
}
