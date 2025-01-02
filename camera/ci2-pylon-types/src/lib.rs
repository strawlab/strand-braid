#[derive(Clone, Debug)]
pub struct PylonExtra {
    pub block_id: u64,
    pub device_timestamp: u64,
}

impl ci2::BackendData for PylonExtra {}
