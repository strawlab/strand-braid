use super::FirehoseImageData;

#[derive(Clone, PartialEq)]
pub struct VideoData {
    inner: Option<FirehoseImageData>,
}

impl VideoData {
    pub fn new(data: FirehoseImageData) -> Self {
        Self { inner: Some(data) }
    }

    pub fn frame_number(&self) -> Option<u64> {
        self.inner.as_ref().map(|x| x.fno)
    }

    pub fn inner(self) -> Option<FirehoseImageData> {
        self.inner
    }
}

impl Default for VideoData {
    fn default() -> Self {
        Self { inner: None }
    }
}
