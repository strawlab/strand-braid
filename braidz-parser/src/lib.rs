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

use flydra_types::{
    FlydraFloatTimestampLocal, HostClock, TextlogRow, TrackingParams,
    RECONSTRUCT_LATENCY_LOG_FNAME, REPROJECTION_DIST_LOG_FNAME,
};

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
    // #[error("HDR Histogram log iterator error {source:?}")]
    // HdrHistogram{source: hdrhistogram::serialization::interval_log::LogIteratorError, #[cfg(feature = "backtrace")]
    // backtrace: Backtrace,},
    // Xml(serde_xml_rs::Error),
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

// impl From<hdrhistogram::serialization::interval_log::LogIteratorError> for Error {
//     fn from(source: hdrhistogram::serialization::interval_log::LogIteratorError) -> Error {
//         Error::HdrHistogram{
//             source,
//             #[cfg(feature = "backtrace")]
//             backtrace: Backtrace::capture(),
//         }
//     }
// }

impl From<serde_xml_rs::Error> for Error {
    fn from(_source: serde_xml_rs::Error) -> Error {
        Error::Xml
    }
}

pub fn file_error<E>(what: &'static str, filename: String, source: E) -> Error
where
    E: 'static + std::error::Error + Sync + Send,
{
    Error::FileError {
        what,
        filename,
        source: Box::new(source),
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace::capture(),
    }
}

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
    pub time_limits: [chrono::DateTime<chrono::Local>; 2],
    pub num_rows: u64,
}

pub struct Seq2d {
    pub frame: Vec<i64>,
    pub xdata: Vec<f64>,
    pub ydata: Vec<f64>,
    pub max_pixel: f64,
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
            max_pixel: 0.0,
        }
    }

    fn push(&mut self, f: i64, x: f64, y: f64) {
        if !x.is_nan() {
            self.frame.push(f);
            self.xdata.push(x);
            self.ydata.push(y);
            self.max_pixel = max(self.max_pixel, max(x, y));
        }
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
    let reader = BufReader::new(std::fs::File::open(&path)?);
    let zs = zip_or_dir::ZipDirArchive::from_zip(reader, path.as_ref().display().to_string())?;
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

/// Pick the `.csv` file (if it exists) as first choice, else pick `.csv.gz`.
///
/// Note, use caution if using `csv_fname` after this, as it may be the original
/// (`.csv`) or new (`.csv.gz`).
pub fn pick_csvgz_or_csv2<'a, R: Read + Seek>(
    csv_fname: &'a mut zip_or_dir::PathLike<R>,
) -> Result<Box<dyn Read + 'a>, Error> {
    if csv_fname.exists() {
        Ok(Box::new(csv_fname.open()?))
    } else {
        csv_fname.set_extension("csv.gz");

        let displayname = format!("{}", csv_fname.display());

        let gz_fd = csv_fname
            .open()
            .map_err(|e| file_error("opening", displayname, e))?;
        Ok(Box::new(libflate::gzip::Decoder::new(gz_fd)?))
    }
}
