extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate chrono;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct RecordingPath {
    path: String,
    start_time: chrono::DateTime<chrono::Utc>,
    current_size_bytes: Option<usize>,
}

impl RecordingPath {
    pub fn new(path: String) -> Self {
        let start_time = chrono::Utc::now();
        RecordingPath::from_path_and_time(path, start_time)
    }
    pub fn from_path_and_time(path: String, start_time: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            path,
            start_time,
            current_size_bytes: None,
        }
    }
    pub fn path(&self) -> String {
        self.path.clone()
    }
    pub fn start_time(&self) -> chrono::DateTime<chrono::Utc> {
        self.start_time
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct ClockModel {
    pub gain: f64,
    pub offset: f64,
    pub residuals: f64,
    pub n_measurements: u64,
}
