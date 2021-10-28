#![cfg_attr(feature = "backtrace", feature(backtrace))]

#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

use std::{
    collections::BTreeMap,
    convert::TryInto,
    fs::File,
    io::{BufReader, Read, Seek},
};

use hdrhistogram::serialization::interval_log;
use ordered_float::NotNan;

use flydra_types::{FlydraFloatTimestampLocal, HostClock, TextlogRow, TrackingParams, Triggerbox};

use braidz_types::{
    BraidMetadata, BraidzSummary, CalibrationInfo, CamInfo, CamInfoRow, CamNum, Data2dDistortedRow,
    Data2dSummary, HistogramSummary, KalmanEstimatesRow, KalmanEstimatesSummary,
};

use csv_eof::EarlyEofOk;

pub mod incremental_parser;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{source}")]
    Mvg {
        #[from]
        source: mvg::MvgError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Io {
        #[from]
        source: std::io::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Zip {
        #[from]
        source: zip::result::ZipError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Yaml {
        #[from]
        source: serde_yaml::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Json {
        #[from]
        source: serde_json::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Csv {
        #[from]
        source: csv::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("XML error")]
    Xml,
    #[error("{source}")]
    ZipOrDir {
        #[from]
        source: zip_or_dir::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    ParseFloat {
        #[from]
        source: std::num::ParseFloatError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("Compressed and uncompressed data copies exist simultaneously")]
    DualData,
    #[error("Multiple tracking parameters")]
    MultipleTrackingParameters,
    #[error("Missing tracking parameters")]
    MissingTrackingParameters,
    #[error("Error opening {filename}: {source}")]
    FileError {
        what: &'static str,
        filename: String,
        source: Box<dyn std::error::Error + Sync + Send>,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
}

impl From<serde_xml_rs::Error> for Error {
    fn from(_source: serde_xml_rs::Error) -> Error {
        Error::Xml
    }
}

// pub fn file_error<E>(what: &'static str, filename: String, source: E) -> Error
// where
//     E: 'static + std::error::Error + Sync + Send,
// {
//     Error::FileError {
//         what,
//         filename,
//         source: Box::new(source),
//         #[cfg(feature = "backtrace")]
//         backtrace: Backtrace::capture(),
//     }
// }

/// The entire file contents, loaded to memory.
///
/// Currently, the implementation does not load everything to memory, but it
/// should and will do so in the future. To load only a summary, use the
/// `BraidzSummary` type. Currently, a summary can only be made by loading the
/// entire archive first, but more efficient path can be made later.
pub struct BraidzArchive<R: Read + Seek> {
    archive: zip_or_dir::ZipDirArchive<R>, //incremental_parser::IncrementalParser<R, incremental_parser::FullyParsed>,
    pub metadata: BraidMetadata,
    pub expected_fps: f64,
    pub calibration_info: Option<CalibrationInfo>,
    pub kalman_estimates_info: Option<KalmanEstimatesInfo>, // TODO: rename to kalman_estimates
    pub reconstruction_latency_hlog: Option<HistogramLog>,
    pub reprojection_distance_hlog: Option<HistogramLog>,
    pub cam_info: CamInfo,
    pub data2d_distorted: Option<D2DInfo>,
}

pub struct HistogramLog {
    histogram: hdrhistogram::Histogram<u64>,
}

impl From<&HistogramLog> for HistogramSummary {
    fn from(orig: &HistogramLog) -> Self {
        HistogramSummary {
            len: orig.histogram.len(),
            mean: orig.histogram.mean(),
            min: orig.histogram.min(),
            max: orig.histogram.max(),
        }
    }
}

impl<R: Read + Seek> BraidzArchive<R> {
    /// Consume and return the raw storage archive.
    pub fn zip_struct(self) -> zip_or_dir::ZipDirArchive<R> {
        self.archive
    }
}

pub struct D2DInfo {
    pub qz: BTreeMap<CamNum, Seq2d>,
    pub frame_lim: [u64; 2],
    pub time_limits: [chrono::DateTime<chrono::Utc>; 2],
    pub num_rows: u64,
}

/// Column store for 2D detections for a single camera.
///
/// Note that these are not filled when there is no detection.
pub struct Seq2d {
    /// The frame number in the synchronized, global frame count.
    pub frame: Vec<i64>,
    /// The x coordinate of the detections.
    pub xdata: Vec<NotNan<f64>>,
    /// The y coordinate of the detections.
    pub ydata: Vec<NotNan<f64>>,
    /// The maximum value of all x and y coordinates.
    pub max_pixel: NotNan<f64>,
    /// The time at which the hardware trigger was computed to have fired to
    /// initiate image acquisition at this frame.
    ///
    /// This is computed based on a model of the clock running on the triggerbox
    /// and keeping this model updated via continual sampling of both the
    /// triggerbox clock and the system clock of the computer hosting the
    /// triggerbox.
    pub timestamp_trigger: Vec<Option<FlydraFloatTimestampLocal<Triggerbox>>>,
    /// The time at which the image was available to the system clock of the
    /// host computer of the camera.
    pub timestamp_host: Vec<FlydraFloatTimestampLocal<HostClock>>,
}

// TODO: rename KalmanEstimates? or ..Data?
pub struct KalmanEstimatesInfo {
    pub xlim: [f64; 2],
    pub ylim: [f64; 2],
    pub zlim: [f64; 2],
    pub trajectories: BTreeMap<u32, TrajectoryData>,
    pub num_rows: u64,
    pub tracking_parameters: TrackingParams,
    /// The sum of all distances in all trajectories.
    pub total_distance: f64,
}

pub struct TrajectoryData {
    pub position: Vec<[f32; 3]>,
    pub start_frame: u64,
    pub distance: f64,
}

impl Seq2d {
    fn new() -> Self {
        Self {
            frame: vec![],
            xdata: vec![],
            ydata: vec![],
            max_pixel: NotNan::new(0.0).unwrap(),
            timestamp_trigger: vec![],
            timestamp_host: vec![],
        }
    }

    fn push(
        &mut self,
        f: i64,
        x: NotNan<f64>,
        y: NotNan<f64>,
        timestamp_trigger: Option<FlydraFloatTimestampLocal<Triggerbox>>,
        timestamp_host: FlydraFloatTimestampLocal<HostClock>,
    ) {
        self.frame.push(f);
        self.xdata.push(x);
        self.ydata.push(y);
        self.timestamp_trigger.push(timestamp_trigger);
        self.timestamp_host.push(timestamp_host);
        self.max_pixel = NotNan::new(max(*self.max_pixel, max(*x, *y))).unwrap();
    }
}

pub fn summarize_braidz<R: Read + Seek>(
    braidz_archive: &BraidzArchive<R>,
    filename: String,
    filesize: u64,
) -> BraidzSummary {
    let data2d_summary = braidz_archive.data2d_distorted.as_ref().map(Into::into);
    let kalman_estimates_summary = braidz_archive
        .kalman_estimates_info
        .as_ref()
        .map(Into::into);

    let reconstruct_latency_usec_summary = braidz_archive
        .reconstruction_latency_hlog
        .as_ref()
        .map(Into::into);

    let reprojection_distance_100x_pixels_summary = braidz_archive
        .reprojection_distance_hlog
        .as_ref()
        .map(Into::into);

    BraidzSummary {
        metadata: braidz_archive.metadata.clone(),
        calibration_info: braidz_archive.calibration_info.clone(),
        expected_fps: braidz_archive.expected_fps,
        cam_info: braidz_archive.cam_info.clone(),
        filename,
        filesize,
        kalman_estimates_summary,
        data2d_summary,
        reconstruct_latency_usec_summary,
        reprojection_distance_100x_pixels_summary,
    }
}

pub fn braidz_parse_path<P: AsRef<std::path::Path>>(
    path: P,
) -> Result<BraidzArchive<BufReader<File>>, Error> {
    let zs = zip_or_dir::ZipDirArchive::auto_from_path(path)?;
    let parsed = braidz_parse(zs)?;
    Ok(parsed)
}

pub fn braidz_parse<R: Read + Seek>(
    archive: zip_or_dir::ZipDirArchive<R>,
) -> Result<BraidzArchive<R>, Error> {
    let ip = incremental_parser::IncrementalParser::from_archive(archive);
    let ip = ip.parse_everything()?;
    let state = ip.state;
    let archive = ip.archive;

    Ok(BraidzArchive {
        archive,
        metadata: state.metadata,
        expected_fps: state.expected_fps,
        calibration_info: state.calibration_info,
        cam_info: state.cam_info,
        kalman_estimates_info: state.kalman_estimates_info,
        data2d_distorted: state.data2d_distorted,
        reconstruction_latency_hlog: state.reconstruction_latency_hlog,
        reprojection_distance_hlog: state.reprojection_distance_hlog,
    })
}

impl<R: Read + Seek> BraidzArchive<R> {
    /// Iterate over the rows of the `data2d_distorted` table.
    ///
    /// This takes a mutable reference because the read location in the archive
    /// is changed during operation.
    pub fn iter_data2d_distorted(
        &mut self,
    ) -> Result<impl Iterator<Item = Result<Data2dDistortedRow, csv::Error>> + '_, Error> {
        let mut data_fname = self.archive.path_starter();
        data_fname.push(flydra_types::DATA2D_DISTORTED_CSV_FNAME);
        let rdr = open_maybe_gzipped(data_fname)?;
        let rdr2 = csv::Reader::from_reader(rdr);
        Ok(rdr2.into_deserialize().early_eof_ok())
    }
}

fn get_hlog<R: Read>(mut rdr: R) -> Result<Option<HistogramLog>, ()> {
    /*
    # Python reader
    from hdrh.histogram import HdrHistogram
    from hdrh.log import HistogramLogReader
    h=HdrHistogram(1,100000,2)
    rdr = HistogramLogReader('reprojection_distance_100x_pixels.hlog', h)
    h1 = rdr.get_next_interval_histogram()
    print(h1.get_total_count())"
    */

    let mut buf = vec![];
    rdr.read_to_end(&mut buf).map_err(|_| ())?;

    let iter = interval_log::IntervalLogIterator::new(&buf);

    use hdrhistogram::{
        serialization::{interval_log::LogEntry, Deserializer},
        Histogram,
    };

    let mut deserializer = Deserializer::new();
    let mut result: Option<Histogram<u64>> = None;

    for interval in iter {
        let interval = interval.map_err(|_| ())?;
        match interval {
            LogEntry::Interval(ilh) => {
                let serialized_histogram =
                    base64::decode_config(ilh.encoded_histogram(), base64::STANDARD)
                        .map_err(|_| ())?;
                let decoded_hist: Histogram<u64> = deserializer
                    .deserialize(&mut std::io::Cursor::new(&serialized_histogram))
                    .map_err(|_| ())?;
                result = match result {
                    Some(mut x) => {
                        x.add(&decoded_hist).map_err(|_| ())?;
                        Some(x)
                    }
                    None => Some(decoded_hist),
                };
            }
            LogEntry::BaseTime(_) | LogEntry::StartTime(_) => {}
        }
    }

    Ok(result.map(|histogram| HistogramLog { histogram }))
}

impl From<&KalmanEstimatesInfo> for KalmanEstimatesSummary {
    fn from(orig: &KalmanEstimatesInfo) -> Self {
        Self {
            num_rows: orig.num_rows,
            x_limits: orig.xlim,
            y_limits: orig.ylim,
            z_limits: orig.zlim,
            num_trajectories: orig.trajectories.len().try_into().unwrap(),
            tracking_parameters: orig.tracking_parameters.clone(),
            total_distance: orig.total_distance,
        }
    }
}

impl From<&D2DInfo> for Data2dSummary {
    fn from(orig: &D2DInfo) -> Self {
        let num_cameras_with_data = orig.qz.len().try_into().unwrap();
        Self {
            time_limits: orig.time_limits,
            frame_limits: orig.frame_lim,
            num_cameras_with_data,
            num_rows: orig.num_rows,
        }
    }
}

fn min(a: f64, b: f64) -> f64 {
    if a > b {
        b
    } else {
        a
    }
}

fn max(a: f64, b: f64) -> f64 {
    if a < b {
        b
    } else {
        a
    }
}

/// Append a suffix to a path.
fn append_to_path(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut s1: std::ffi::OsString = path.to_path_buf().into_os_string(); // copy data
    s1.push(suffix);
    s1.into()
}

#[test]
fn test_append_to_path() {
    let foo = std::path::Path::new("foo");
    assert!(append_to_path(foo, ".gz") == std::path::Path::new("foo.gz"));

    let foo_csv = std::path::Path::new("foo.csv");
    assert!(append_to_path(foo_csv, ".gz") == std::path::Path::new("foo.csv.gz"));
}

/// Pick the `.csv` file (if it exists) as first choice, else pick `.csv.gz`.
///
/// Note, use caution if using `csv_fname` after this, as it may be the original
/// (`.csv`) or new (`.csv.gz`).
pub fn open_maybe_gzipped<'a, R: Read + Seek>(
    mut path_like: zip_or_dir::PathLike<'a, R>,
) -> Result<Box<dyn Read + 'a>, Error> {
    let compressed_relname = append_to_path(path_like.path(), ".gz");

    if path_like.exists() {
        const CHECK_NO_DUAL_DATA: bool = true;
        if CHECK_NO_DUAL_DATA {
            // Check the compressed variant does not exist. Due to reasons, we
            // have replace, but not clone, so we replace the original with the
            // new and then back again.
            let uncompressed_relname = path_like.replace(compressed_relname);
            if path_like.exists() {
                return Err(Error::DualData);
            }
            path_like.replace(uncompressed_relname);
        }
        Ok(Box::new(path_like.open()?))
    } else {
        // Use the compressed variant.
        path_like.replace(compressed_relname);
        let gz_fd = path_like.open()?;
        Ok(Box::new(libflate::gzip::Decoder::new(gz_fd)?))
    }
}
