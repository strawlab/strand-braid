//! This is an incremental parser for braid archives.

use crate::*;

/// The implementation specifies in what state we are in terms of parsing an archive.
pub trait ParseState {}

/// The archive has just been opened.
pub struct ArchiveOpened {}

/// The archive has basic information parsed.
// The Result<> types store an error indicating why the field was not loaded.
#[derive(Debug)]
pub struct BasicInfoParsed {
    pub metadata: BraidMetadata,
    pub expected_fps: f64,
    pub tracking_params: Option<TrackingParams>,
    pub calibration_info: Option<CalibrationInfo>,
    pub reconstruction_latency_hlog: Option<HistogramLog>,
    pub reprojection_distance_hlog: Option<HistogramLog>,
    pub cam_info: CamInfo,
}

/// The archive been completely parsed.
#[derive(Debug)]
pub struct FullyParsed {
    pub metadata: BraidMetadata,
    pub expected_fps: f64,
    pub calibration_info: Option<CalibrationInfo>,
    pub kalman_estimates_info: Option<KalmanEstimatesInfo>, // TODO: rename to kalman_estimates
    pub kalman_estimates_table: Option<Vec<KalmanEstimatesRow>>,
    pub reconstruction_latency_hlog: Option<HistogramLog>,
    pub reprojection_distance_hlog: Option<HistogramLog>,
    pub cam_info: CamInfo,
    pub data2d_distorted: Option<D2DInfo>,
    /// A mapping from camera name to (width, height).
    pub image_sizes: Option<BTreeMap<String, (usize, usize)>>,
}

impl ParseState for ArchiveOpened {}
impl ParseState for BasicInfoParsed {}
impl ParseState for FullyParsed {}

/// An incremental parser for braid archives.
///
/// Initially, minimal reading from the archive is performed. As further
/// operations on the archive proceed, the state of the parser gradually
/// accumulates more information.
// TODO: change this to an enum which changes its variant as it reads more.
pub struct IncrementalParser<R: Read + Seek, S: ParseState> {
    pub(crate) archive: zip_or_dir::ZipDirArchive<R>,
    /// The state of parsing. Storage for stage-specific data.
    pub(crate) state: S,
}

impl<R: Read + Seek, S: ParseState> std::fmt::Debug for IncrementalParser<R, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.debug_struct("IncrementalParser")
            .field("archive", &self.archive)
            .finish_non_exhaustive()
    }
}

impl IncrementalParser<BufReader<std::fs::File>, ArchiveOpened> {
    /// Open an archive from a path.
    ///
    /// The archive may be a .braidz zip file for a .braid directory.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Self, Error> {
        let archive = zip_or_dir::ZipDirArchive::auto_from_path(path)?;
        Ok(Self::from_archive(archive))
    }

    /// Open an archive from a directory.
    ///
    /// The archive must be a .braid directory.
    pub fn open_dir<P: AsRef<std::path::Path>>(path: P) -> Result<Self, Error> {
        let archive = zip_or_dir::ZipDirArchive::from_dir(path.as_ref().to_path_buf())?;
        Ok(Self::from_archive(archive))
    }

    /// Open an archive from a .braidz zip file.
    ///
    /// The archive must be a .braidz zip file.
    pub fn open_braidz_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self, Error> {
        let reader = BufReader::new(std::fs::File::open(&path)?);
        let archive = zip_or_dir::ZipDirArchive::from_zip(
            reader,
            path.as_ref().as_os_str().to_str().unwrap().to_string(),
        )?;
        Ok(Self::from_archive(archive))
    }
}

impl<R: Read + Seek> IncrementalParser<R, ArchiveOpened> {
    /// Open an archive.
    ///
    /// The archive may be a .braidz zip file for a .braid directory.
    pub fn from_archive(archive: zip_or_dir::ZipDirArchive<R>) -> Self {
        IncrementalParser {
            archive,
            state: ArchiveOpened {},
        }
    }

    /// Parse the basic data which can be quickly read from the archive.
    pub fn parse_basics(mut self) -> Result<IncrementalParser<R, BasicInfoParsed>, Error> {
        let mut metadata: Option<BraidMetadata> = {
            match self.archive.open(braid_types::BRAID_METADATA_YML_FNAME) {
                Ok(rdr) => Some(serde_yaml::from_reader(rdr)?),
                Err(zip_or_dir::Error::FileNotFound) => None,
                Err(e) => {
                    return Err(Error::FileError {
                        source: Box::new(e),
                        filename: braid_types::BRAID_METADATA_YML_FNAME.into(),
                        what: "opening metadata file",
                    })
                }
            }
        };

        // should match:
        //  - "unknown fps, (flydra_version 2.0.0, git_revision 0581c8fa8da4e683921480085fad21bf3b77600e, time_tzname0 CEST)"
        //  - "100.0 fps, (top 10000, hypothesis_test_max_error 20.0)"
        //  - "100.0 fps, (flydra_version 0.6.7, time_tzname0 CET)"
        //  - "20.1 fps, ()"

        let re_fps = regex::Regex::new(r"^(\S+) fps, \((.*)\)$").unwrap();

        let re_inner =
            regex::Regex::new(r"flydra_version (.+)\S, git_revision (\w+), time_tzname0 (.+)")
                .unwrap();

        // Parse fps and tracking parameters from textlog.
        let mut expected_fps = f64::NAN;
        let tracking_params: Option<TrackingParams> = {
            let mut fname = self.archive.path_starter();
            fname.push(braid_types::TEXTLOG_CSV_FNAME);
            let tracking_parameters = match open_maybe_gzipped(fname) {
                Ok(rdr) => {
                    let mut tracking_parameters = None;
                    let textlog_rdr = csv::Reader::from_reader(rdr);
                    for (rownum, row) in textlog_rdr.into_deserialize().early_eof_ok().enumerate() {
                        let row: TextlogRow = row?;

                        tracing::debug!(
                            "Line in {} (row {}): {}",
                            braid_types::TEXTLOG_CSV_FNAME,
                            rownum,
                            row.message
                        );

                        // TODO: fix DRY in `calc_fps_from_data()`.
                        let line1_start = "MainBrain running at ";

                        if let Some(line1_data) = row.message.strip_prefix(line1_start) {
                            let caps = match re_fps.captures(line1_data) {
                                Some(caps) => caps,
                                None => return Err(Error::UnknownTextlogData),
                            };
                            let fps_str = caps.get(1).unwrap().as_str();
                            let inner_str = caps.get(2).unwrap().as_str();
                            let git_revision = match re_inner.captures(inner_str) {
                                Some(caps2) => caps2.get(2).unwrap().as_str().to_string(),
                                None => "unknown".to_string(),
                            };

                            if fps_str != "unknown" {
                                expected_fps = fps_str.parse()?;
                            }

                            if metadata.is_none() {
                                let timestamp = strand_datetime_conversion::f64_to_datetime(
                                    row.mainbrain_timestamp,
                                );

                                let local: chrono::DateTime<chrono::Local> =
                                    timestamp.with_timezone(&chrono::Local);

                                metadata = Some(BraidMetadata {
                                    git_revision,
                                    original_recording_time: Some(local),
                                    saving_program_name: "flydra".to_string(),
                                    schema: braid_types::BRAID_SCHEMA,
                                    save_empty_data2d: false,
                                });
                            }

                            // No more parsing of this line. In particular, it
                            // is not JSON.
                            continue;
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
                                    let tp: TrackingParams =
                                        serde_json::from_value(params_js_value)?;
                                    if tracking_parameters.is_some() {
                                        return Err(Error::MultipleTrackingParameters);
                                    }
                                    tracking_parameters = Some(tp);
                                }
                            }
                            Err(_e) => {
                                // Cannot parse as JSON, but this is not a fatal problem.
                                tracing::warn!(
                                    "cannot parse message in textlog (row {rownum}) as JSON"
                                );
                            }
                        }
                    }

                    tracking_parameters
                }
                Err(_e) => None,
            };
            tracking_parameters
        };

        let metadata = if let Some(metadata) = metadata {
            metadata
        } else {
            return Err(Error::MissingMetadata {});
        };

        let calibration_info = {
            match self.archive.open(braid_types::CALIBRATION_XML_FNAME) {
                Ok(rdr) => {
                    let recon: flydra_mvg::flydra_xml_support::FlydraReconstructor<f64> =
                        serde_xml_rs::from_reader(rdr)?;

                    let system =
                        flydra_mvg::FlydraMultiCameraSystem::from_flydra_reconstructor(&recon)?;
                    Some(CalibrationInfo {
                        water: recon.water,
                        cameras: system.to_system(),
                    })
                }
                Err(zip_or_dir::Error::FileNotFound) => None,
                Err(e) => {
                    return Err(Error::FileError {
                        source: Box::new(e),
                        filename: braid_types::CALIBRATION_XML_FNAME.into(),
                        what: "opening calibration file",
                    })
                }
            }
        };

        let reconstruction_latency_hlog = {
            let reconstruction_latency_hlog = match self
                .archive
                .open(braid_types::RECONSTRUCT_LATENCY_HLOG_FNAME)
            {
                Ok(rdr) => get_hlog(rdr).unwrap(),
                Err(zip_or_dir::Error::FileNotFound) => None,
                Err(e) => return Err(e.into()),
            };
            reconstruction_latency_hlog
        };

        let reprojection_distance_hlog = {
            let reprojection_distance_hlog =
                match self.archive.open(braid_types::REPROJECTION_DIST_HLOG_FNAME) {
                    Ok(rdr) => get_hlog(rdr).unwrap(),
                    Err(zip_or_dir::Error::FileNotFound) => None,
                    Err(e) => return Err(e.into()),
                };
            reprojection_distance_hlog
        };

        let cam_info = {
            let mut fname = self.archive.path_starter();
            fname.push(braid_types::CAM_INFO_CSV_FNAME);
            let rdr = open_maybe_gzipped(fname)?;
            let caminfo_rdr = csv::Reader::from_reader(rdr);
            let mut camn2camid = BTreeMap::new();
            let mut camid2camn = BTreeMap::new();
            for row in caminfo_rdr.into_deserialize().early_eof_ok() {
                let row: CamInfoRow = row?;
                camn2camid.insert(row.camn, row.cam_id.clone());
                camid2camn.insert(row.cam_id, row.camn);
            }
            CamInfo {
                camn2camid,
                camid2camn,
            }
        };

        let state = BasicInfoParsed {
            metadata,
            expected_fps,
            tracking_params,
            calibration_info,
            reconstruction_latency_hlog,
            reprojection_distance_hlog,
            cam_info,
        };

        Ok(IncrementalParser {
            archive: self.archive,
            state,
        })
    }

    /// Parse the entire archive.
    pub fn parse_everything(self) -> Result<IncrementalParser<R, FullyParsed>, Error> {
        let basics = self.parse_basics()?;
        basics.parse_rest()
    }
}

impl<R: Read + Seek> IncrementalParser<R, BasicInfoParsed> {
    /// Parse the remaining aspects of the archive.
    pub fn parse_rest(mut self) -> Result<IncrementalParser<R, FullyParsed>, Error> {
        let basics = self.state;

        let mut num_rows = 0;
        let mut limits: Option<([u64; 2], [FlydraFloatTimestampLocal<HostClock>; 2])> = None;

        let qz = {
            // Open main 2D data.
            let mut data_fname = self.archive.path_starter();
            data_fname.push(braid_types::DATA2D_DISTORTED_CSV_FNAME);
            let rdr = open_maybe_gzipped(data_fname)?;
            let d2d_reader = csv::Reader::from_reader(rdr);
            let mut qz = BTreeMap::new();

            for row in d2d_reader.into_deserialize().early_eof_ok() {
                num_rows += 1;
                let row: Data2dDistortedRow = row?;
                let entry = qz.entry(row.camn).or_insert_with(Seq2d::new);
                if let Ok(x) = NotNan::new(row.x) {
                    // Iff x is NaN, so is y.
                    let y = NotNan::new(row.y).unwrap();
                    // If 2d detection data was NaN, ignore it.
                    entry.push(
                        row.frame,
                        x,
                        y,
                        row.timestamp,
                        row.cam_received_timestamp.clone(),
                    );
                }
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

        let (kalman_estimates_info, kalman_estimates_table) = {
            let mut fname = self.archive.path_starter();
            fname.push(braid_types::KALMAN_ESTIMATES_CSV_FNAME);
            let mut kalman_estimates_table = Vec::new();
            match open_maybe_gzipped(fname) {
                Ok(rdr) => {
                    let kest_reader = csv::Reader::from_reader(rdr);
                    let mut trajectories = BTreeMap::new();
                    let inf = 1.0 / 0.0;
                    let mut xlim = [inf, -inf];
                    let mut ylim = [inf, -inf];
                    let mut zlim = [inf, -inf];
                    let mut num_rows = 0;

                    for row in kest_reader.into_deserialize().early_eof_ok() {
                        let row: KalmanEstimatesRow = row?;
                        let entry =
                            trajectories
                                .entry(row.obj_id)
                                .or_insert_with(|| TrajectoryData {
                                    // Initialize the structure with empty position vector
                                    // and zero distance.
                                    position: Vec::new(),
                                    start_frame: row.frame.0,
                                    distance: 0.0,
                                });
                        entry
                            .position
                            .push([row.x as f32, row.y as f32, row.z as f32]);

                        xlim[0] = min(xlim[0], row.x);
                        xlim[1] = max(xlim[1], row.x);
                        ylim[0] = min(ylim[0], row.y);
                        ylim[1] = max(ylim[1], row.y);
                        zlim[0] = min(zlim[0], row.z);
                        zlim[1] = max(zlim[1], row.z);
                        num_rows += 1;
                        kalman_estimates_table.push(row);
                    }

                    let mut total_distance: f64 = 0.0;
                    // Loop through all individual trajectories and calculate the
                    // distance per trajectory.
                    for (_obj_id, traj_data) in trajectories.iter_mut() {
                        let mut previous: Option<&[f32; 3]> = None;
                        for current in traj_data.position.iter() {
                            if let Some(previous) = previous {
                                let dx: f64 = (current[0] - previous[0]).into();
                                let dy: f64 = (current[1] - previous[1]).into();
                                let dz: f64 = (current[2] - previous[2]).into();
                                traj_data.distance += (dx.powi(2) + dy.powi(2) + dz.powi(2)).sqrt();
                            }
                            previous = Some(current);
                        }
                        // Accumulate total distance of all trajectories.
                        total_distance += traj_data.distance;
                    }

                    let tracking_parameters = match basics.tracking_params {
                        Some(tp) => tp,
                        None => {
                            return Err(Error::MissingTrackingParameters);
                        }
                    };

                    (
                        Some(KalmanEstimatesInfo {
                            xlim,
                            ylim,
                            zlim,
                            trajectories,
                            num_rows,
                            tracking_parameters,
                            total_distance,
                        }),
                        Some(kalman_estimates_table),
                    )
                }
                Err(e) =>
                {
                    #[allow(unused_variables)]
                    match e {
                        Error::ZipOrDir {
                            source: zip_or_dir::Error::FileNotFound,
                        } => (None, None),
                        _ => {
                            return Err(e);
                        }
                    }
                }
            }
        };

        let image_sizes = if let Some(calibration_info) = basics.calibration_info.as_ref() {
            Some(
                calibration_info
                    .cameras
                    .cams_by_name()
                    .iter()
                    .map(|(k, v)| (k.clone(), (v.width(), v.height())))
                    .collect(),
            )
        } else {
            let mut result: BTreeMap<String, (usize, usize)> = Default::default();
            let mut failed = false;
            for cam_id in basics.cam_info.camid2camn.keys() {
                let relname = format!("{}/{cam_id}.png", braid_types::IMAGES_DIRNAME);
                match self.archive.open(relname) {
                    Ok(mut rdr) => {
                        let mut buf = Vec::new();
                        rdr.read_to_end(&mut buf)?;
                        let cur = std::io::Cursor::new(buf);
                        let decoder = image::codecs::png::PngDecoder::new(cur)?;
                        let (w, h) = image::ImageDecoder::dimensions(&decoder);
                        result.insert(cam_id.clone(), (w as usize, h as usize));
                    }
                    Err(zip_or_dir::Error::FileNotFound) => {
                        failed = true;
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            if !failed {
                Some(result)
            } else {
                None
            }
        };

        let cam_info = basics.cam_info;

        Ok(IncrementalParser {
            archive: self.archive,
            state: FullyParsed {
                metadata: basics.metadata,
                expected_fps: basics.expected_fps,
                calibration_info: basics.calibration_info,
                cam_info,
                kalman_estimates_info,
                kalman_estimates_table,
                data2d_distorted,
                reconstruction_latency_hlog: basics.reconstruction_latency_hlog,
                reprojection_distance_hlog: basics.reprojection_distance_hlog,
                image_sizes,
            },
        })
    }

    pub fn basic_info(&self) -> &BasicInfoParsed {
        &self.state
    }
}

impl<R: Read + Seek> IncrementalParser<R, FullyParsed> {
    pub fn kalman_estimates_info(&self) -> Option<&KalmanEstimatesInfo> {
        self.state.kalman_estimates_info.as_ref()
    }
}

impl<R: Read + Seek, S: ParseState> IncrementalParser<R, S> {
    /// Consume and return the raw storage archive.
    pub fn into_inner(self) -> zip_or_dir::ZipDirArchive<R> {
        self.archive
    }

    /// Display the path of the archive.
    pub fn display(&self) -> std::path::Display<'_> {
        self.archive.display()
    }

    /// Get a path-like instance for direct read access to the archive.
    ///
    /// You should prefer to use information already parsed from the archive
    /// rather than resorting to this low-level function. Consider expanding the
    /// parser to provide this information if it is not already implemented.
    pub fn path_starter(&mut self) -> zip_or_dir::PathLike<'_, R> {
        self.archive.path_starter()
    }
}
