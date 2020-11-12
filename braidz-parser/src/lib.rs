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
    BraidMetadata, BraidzSummary, CalibrationInfo, CamInfoRow, CamNum, Data2dDistortedRow,
    Data2dSummary, HistogramSummary, KalmanEstimatesRow, KalmanEstimatesSummary,
};
use csv_eof::EarlyEofOk;

#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

#[derive(Debug)]
pub enum ErrorKind {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    Yaml(serde_yaml::Error),
    Json(serde_json::Error),
    Csv(csv::Error),
    HdrHistogram(hdrhistogram::serialization::interval_log::LogIteratorError),
    // Xml(serde_xml_rs::Error),
    Xml,
    ZipOrDir(zip_or_dir::Error),
    ParseFloat(std::num::ParseFloatError),
    MultipleTrackingParameters,
    MissingTrackingParameters,
}

impl From<std::io::Error> for Error {
    fn from(orig: std::io::Error) -> Error {
        Error {
            kind: ErrorKind::Io(orig),
        }
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(orig: zip::result::ZipError) -> Error {
        Error {
            kind: ErrorKind::Zip(orig),
        }
    }
}

impl From<serde_yaml::Error> for Error {
    fn from(orig: serde_yaml::Error) -> Error {
        Error {
            kind: ErrorKind::Yaml(orig),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(orig: serde_json::Error) -> Error {
        Error {
            kind: ErrorKind::Json(orig),
        }
    }
}

impl From<csv::Error> for Error {
    fn from(orig: csv::Error) -> Error {
        Error {
            kind: ErrorKind::Csv(orig),
        }
    }
}

impl From<hdrhistogram::serialization::interval_log::LogIteratorError> for Error {
    fn from(orig: hdrhistogram::serialization::interval_log::LogIteratorError) -> Error {
        Error {
            kind: ErrorKind::HdrHistogram(orig),
        }
    }
}

impl From<serde_xml_rs::Error> for Error {
    fn from(_orig: serde_xml_rs::Error) -> Error {
        Error {
            // kind: ErrorKind::Xml(orig),
            kind: ErrorKind::Xml,
        }
    }
}

impl From<zip_or_dir::Error> for Error {
    fn from(orig: zip_or_dir::Error) -> Error {
        Error {
            kind: ErrorKind::ZipOrDir(orig),
        }
    }
}

impl From<std::num::ParseFloatError> for Error {
    fn from(orig: std::num::ParseFloatError) -> Error {
        Error {
            kind: ErrorKind::ParseFloat(orig),
        }
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error { kind }
    }
}

/// The entire file contents, loaded to memory.
///
/// Currently, the implementation does not load everything to memory, but it
/// should. To load only a summary, use the summary types. Currently, a summary
/// can only be made by loading the entire archive first, but more efficient
/// path can be made later.
pub struct BraidzArchive<R: Read + Seek> {
    archive: zip_or_dir::ZipDirArchive<R>,
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

pub struct CamInfo {
    pub camn2camid: BTreeMap<CamNum, String>,
    pub camid2camn: BTreeMap<String, CamNum>,
}

// TODO: rename KalmanEstimates? or ..Data?
pub struct KalmanEstimatesInfo {
    pub xlim: [f64; 2],
    pub ylim: [f64; 2],
    pub zlim: [f64; 2],
    pub trajectories: BTreeMap<u32, Vec<(f32, f32, f32)>>, // TODO: switch to array, not tuple. add frame numbers.
    pub num_rows: u64,
    pub tracking_parameters: TrackingParams,
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
    mut archive: zip_or_dir::ZipDirArchive<R>,
) -> Result<BraidzArchive<R>, Error> {
    let metadata = {
        let file = archive.open("braid_metadata.yml")?;
        serde_yaml::from_reader(file)?
    };

    let mut expected_fps = std::f64::NAN;

    let tracking_parameters: Option<TrackingParams> = {
        match archive.open("textlog.csv.gz") {
            Ok(encoded) => {
                let mut tracking_parameters = None;
                let decoder = libflate::gzip::Decoder::new(encoded)?;
                let kest_reader = csv::Reader::from_reader(decoder);
                for row in kest_reader.into_deserialize().early_eof_ok().into_iter() {
                    let row: TextlogRow = row?;

                    // TODO: combine with `flydra2::offline_kalmanize::calc_fps_from_data()`.
                    let line1_start = "MainBrain running at ";

                    if row.message.starts_with(line1_start) {
                        let line = row.message.replace(line1_start, "");
                        let fps_str = line.split(" ").next().unwrap();
                        expected_fps = fps_str.parse()?;
                    }

                    // parse to unstructured json
                    let js_value_res: Result<serde_json::Value, _> =
                        serde_json::from_str(&row.message);

                    match js_value_res {
                        Ok(mut js_value) => {
                            if js_value
                                .as_object_mut()
                                .unwrap()
                                .contains_key("tracking_params")
                            {
                                // If we have this key, we return an error if we
                                // cannot parse it.
                                let params_js_value = js_value["tracking_params"].take();
                                let tp: TrackingParams = serde_json::from_value(params_js_value)?;
                                if tracking_parameters.is_some() {
                                    return Err(ErrorKind::MultipleTrackingParameters.into());
                                }
                                tracking_parameters = Some(tp);
                            }
                        }
                        Err(_e) => {
                            // Cannot parse as JSON, but this is not a fatal problem.
                            log::warn!("cannot parse message in textlog as JSON");
                        }
                    }
                }
                tracking_parameters
            }
            Err(_e) => None,
        }
    };

    let cam_info = {
        match archive.open("cam_info.csv.gz") {
            Ok(encoded) => {
                let decoder = libflate::gzip::Decoder::new(encoded)?;
                let kest_reader = csv::Reader::from_reader(decoder);
                let mut camn2camid = BTreeMap::new();
                let mut camid2camn = BTreeMap::new();
                for row in kest_reader.into_deserialize().early_eof_ok().into_iter() {
                    let row: CamInfoRow = row?;
                    camn2camid.insert(row.camn, row.cam_id.clone());
                    camid2camn.insert(row.cam_id, row.camn);
                }
                CamInfo {
                    camn2camid,
                    camid2camn,
                }
            }
            Err(e) => return Err(e.into()),
        }
    };

    let mut num_rows = 0;
    let mut limits: Option<([u64; 2], [FlydraFloatTimestampLocal<HostClock>; 2])> = None;
    let qz = match archive.open("data2d_distorted.csv.gz") {
        Ok(encoded) => {
            let decoder = libflate::gzip::Decoder::new(encoded)?;
            let d2d_reader = csv::Reader::from_reader(decoder);
            let mut qz = BTreeMap::new();
            for row in d2d_reader.into_deserialize().early_eof_ok().into_iter() {
                num_rows += 1;
                let row: Data2dDistortedRow = row?;
                let entry = qz.entry(row.camn).or_insert_with(|| Seq2d::new());
                entry.push(row.frame, row.x, row.y);
                let this_frame: u64 = row.frame.try_into().unwrap();
                let this_time = row.cam_received_timestamp;
                if let Some((ref mut f_lim, ref mut time_lim)) = limits {
                    f_lim[0] = std::cmp::min(f_lim[0], this_frame);
                    f_lim[1] = std::cmp::max(f_lim[1], this_frame);
                    time_lim[1] = this_time;
                } else {
                    // Initialize with the first row of data.
                    limits = Some(([this_frame, this_frame], [this_time.clone(), this_time]));
                }
            }
            qz
        }
        Err(e) => return Err(e.into()),
    };

    let data2d_distorted = limits.map(|(frame_lim, tlims)| {
        let time_limits = [(&tlims[0]).into(), (&tlims[1]).into()];
        D2DInfo {
            qz,
            frame_lim,
            time_limits,
            num_rows,
        }
    });

    let calibration_info = match archive.open("calibration.xml") {
        Ok(xml_reader) => {
            let recon: flydra_mvg::flydra_xml_support::FlydraReconstructor<f64> =
                serde_xml_rs::from_reader(xml_reader)?;
            Some(CalibrationInfo { water: recon.water })
        }
        Err(zip_or_dir::Error::FileNotFound) => None,
        Err(e) => return Err(e.into()),
    };

    let kalman_estimates_info = match archive.open("kalman_estimates.csv.gz") {
        Ok(encoded) => {
            let tracking_parameters = match tracking_parameters {
                Some(tp) => tp,
                None => {
                    return Err(ErrorKind::MissingTrackingParameters.into());
                }
            };
            let decoder = libflate::gzip::Decoder::new(encoded)?;
            let kest_reader = csv::Reader::from_reader(decoder);
            let mut trajectories = BTreeMap::new();
            let inf = 1.0 / 0.0;
            let mut xlim = [inf, -inf];
            let mut ylim = [inf, -inf];
            let mut zlim = [inf, -inf];
            let mut num_rows = 0;
            for row in kest_reader.into_deserialize().early_eof_ok().into_iter() {
                let row: KalmanEstimatesRow = row?;
                let entry = trajectories.entry(row.obj_id).or_insert_with(|| Vec::new());
                entry.push((row.x as f32, row.y as f32, row.z as f32));

                xlim[0] = min(xlim[0], row.x);
                xlim[1] = max(xlim[1], row.x);
                ylim[0] = min(ylim[0], row.y);
                ylim[1] = max(ylim[1], row.y);
                zlim[0] = min(zlim[0], row.z);
                zlim[1] = max(zlim[1], row.z);
                num_rows += 1;
            }
            Some(KalmanEstimatesInfo {
                xlim,
                ylim,
                zlim,
                trajectories,
                num_rows,
                tracking_parameters,
            })
        }
        Err(zip_or_dir::Error::FileNotFound) => None,
        Err(e) => return Err(e.into()),
    };

    let reconstruction_latency_hlog = match archive.open(RECONSTRUCT_LATENCY_LOG_FNAME) {
        Ok(rdr) => get_hlog(rdr).unwrap(),
        Err(zip_or_dir::Error::FileNotFound) => None,
        Err(e) => return Err(e.into()),
    };

    let reprojection_distance_hlog = match archive.open(REPROJECTION_DIST_LOG_FNAME) {
        Ok(rdr) => get_hlog(rdr).unwrap(),
        Err(zip_or_dir::Error::FileNotFound) => None,
        Err(e) => return Err(e.into()),
    };

    Ok(BraidzArchive {
        archive,
        metadata,
        expected_fps,
        calibration_info,
        cam_info,
        kalman_estimates_info,
        data2d_distorted,
        reconstruction_latency_hlog,
        reprojection_distance_hlog,
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
