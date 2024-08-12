#[derive(PartialEq)]
pub struct VideoData {
    inner: Option<http_video_streaming_types::ToClient>,
}

impl VideoData {
    pub(crate) fn new(inner: Option<http_video_streaming_types::ToClient>) -> Self {
        Self { inner }
    }
    pub(crate) fn take(&mut self) -> Option<http_video_streaming_types::ToClient> {
        self.inner.take()
    }
}
