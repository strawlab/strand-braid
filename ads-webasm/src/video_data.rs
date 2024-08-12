#[derive(PartialEq)]
pub struct VideoData {
    inner: Option<http_video_streaming_types::ToClient>,
}

impl VideoData {
    pub fn new(inner: Option<http_video_streaming_types::ToClient>) -> Self {
        Self { inner }
    }

    pub fn frame_number(&self) -> Option<u64> {
        let result = self.inner.as_ref().map(|x| x.fno);
        result
    }

    pub fn as_ref(&self) -> Option<&http_video_streaming_types::ToClient> {
        self.inner.as_ref()
    }

    pub fn take(&mut self) -> Option<http_video_streaming_types::ToClient> {
        self.inner.take()
    }
}
