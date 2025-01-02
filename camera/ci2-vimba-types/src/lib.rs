#[derive(Clone, Debug)]
pub struct VimbaExtra {
    pub frame_id: u64,
    pub device_timestamp: u64,
}

impl ci2::BackendData for VimbaExtra {}
