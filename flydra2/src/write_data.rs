use libflate::{finish::AutoFinishUnchecked, gzip::Encoder};
use std::{io::Write, sync::Arc};
use tracing::info;

use braid_types::{
    CamInfoRow, MyFloat, TextlogRow, TrackingParams, BRAID_SCHEMA, CAM_SETTINGS_DIRNAME,
    FEATURE_DETECT_SETTINGS_DIRNAME, IMAGES_DIRNAME, RECONSTRUCT_LATENCY_HLOG_FNAME,
    REPROJECTION_DIST_HLOG_FNAME,
};

use braidz_types::BraidMetadata;

use crate::{
    finish_histogram, histogram_record, save_hlog, ConnectedCamerasManager, ExperimentInfoRow,
    FrameDataAndPoints, HistogramWritingState, KalmanEstimateRecord, OrderingWriter, Result,
    SaveToDiskMsg, StartSavingCsvConfig, TrackingParamsSaver,
};

struct WritingState {
    output_dirname: std::path::PathBuf,
    /// The readme file in the output directory.
    ///
    /// We keep this file open to establish locking on the open directory.
    ///
    /// In theory, we might prefer an open reference to the directory itself,
    /// but this does not seem possible. So we have a potential slight race
    /// condition when we have our directory but not yet the file handle on
    /// readme.
    #[allow(dead_code)]
    readme_fd: Option<std::fs::File>,
    save_empty_data2d: bool,
    // kalman_estimates_wtr: Option<csv::Writer<Box<dyn std::io::Write>>>,
    kalman_estimates_wtr: Option<OrderingWriter>,
    data_assoc_wtr: Option<csv::Writer<Box<dyn std::io::Write + Send>>>,
    data_2d_wtr: csv::Writer<Box<dyn std::io::Write + Send>>,
    textlog_wtr: csv::Writer<Box<dyn std::io::Write + Send>>,
    trigger_clock_info_wtr: csv::Writer<Box<dyn std::io::Write + Send>>,
    experiment_info_wtr: csv::Writer<Box<dyn std::io::Write + Send>>,
    writer_stats: Option<(usize, usize)>,
    file_start_time: std::time::SystemTime,

    reconstruction_latency_usec: Option<HistogramWritingState>,
    reproj_dist_pixels: Option<HistogramWritingState>,
    last_flush: std::time::Instant,
}

fn _test_writing_state_is_send() {
    // Compile-time test to ensure WritingState implements Send trait.
    fn implements<T: Send>() {}
    implements::<WritingState>();
}

#[derive(Clone, Debug)]
pub enum BraidMetadataBuilder {
    GenerateNew(MetadataParts),
    Existing(BraidMetadata),
}

impl BraidMetadataBuilder {
    /// Constructor to help with backwards compatibility
    pub fn saving_program_name<S: Into<String>>(saving_program_name: S) -> BraidMetadataBuilder {
        BraidMetadataBuilder::GenerateNew(MetadataParts {
            saving_program_name: saving_program_name.into(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct MetadataParts {
    saving_program_name: String,
}

impl WritingState {
    fn new(
        cfg: StartSavingCsvConfig,
        cam_info_rows: Vec<CamInfoRow>,
        recon: &Option<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
        tracking_params: Arc<TrackingParams>,
        save_empty_data2d: bool,
        metadata_builder: BraidMetadataBuilder,
    ) -> Result<Self> {
        let output_dirname = cfg.out_dir;
        let local = cfg.local;
        let git_revision = cfg.git_rev;
        let fps = cfg.fps;
        let per_cam_data = cfg.per_cam_data;

        // Any changes to what is saved should update BraidMetadataSchemaTag.

        // create output dir
        std::fs::create_dir_all(&output_dirname)?;

        // Until we obtain the readme file handle, we have a small race
        // condition where another process could also open this directory.

        let readme_fd = {
            let readme_path = output_dirname.join(braid_types::README_MD_FNAME);

            let mut fd = std::fs::File::create(readme_path)?;

            // Start and end it with some newlines so the text is more
            // readable.
            fd.write_all(
                "\n\nThis is data saved by the braid program. \
                See https://strawlab.org/braid for more information.\n\n"
                    .as_bytes(),
            )
            .unwrap();
            Some(fd)
        };

        {
            let braid_metadata_path = output_dirname.join(braid_types::BRAID_METADATA_YML_FNAME);

            let metadata = match metadata_builder {
                BraidMetadataBuilder::GenerateNew(parts) => {
                    BraidMetadata {
                        schema: BRAID_SCHEMA, // BraidMetadataSchemaTag
                        git_revision: git_revision.clone(),
                        original_recording_time: local,
                        save_empty_data2d,
                        saving_program_name: parts.saving_program_name,
                    }
                }
                BraidMetadataBuilder::Existing(metadata) => metadata,
            };
            let metadata_buf = serde_yaml::to_string(&metadata).unwrap();

            let mut fd = std::fs::File::create(braid_metadata_path)?;
            fd.write_all(metadata_buf.as_bytes()).unwrap();
        }

        // write images
        {
            let mut image_path = output_dirname.clone();
            image_path.push(IMAGES_DIRNAME);
            std::fs::create_dir_all(&image_path)?;

            for (raw_cam_name, data) in per_cam_data.iter() {
                let buf = data.current_image_png.as_slice();
                let fname = format!("{}.png", raw_cam_name.as_str());
                let fullpath = image_path.clone().join(fname);
                let mut fd = std::fs::File::create(&fullpath)?;
                fd.write_all(buf)?;
            }
        }

        // write camera settings
        {
            let mut cam_settings_path = output_dirname.clone();
            cam_settings_path.push(CAM_SETTINGS_DIRNAME);
            if per_cam_data
                .iter()
                .any(|(_, x)| x.cam_settings_data.is_some())
            {
                std::fs::create_dir_all(&cam_settings_path)?;
            }

            for (raw_cam_name, cam) in per_cam_data.iter() {
                if let Some(data) = &cam.cam_settings_data {
                    let fname = format!(
                        "{}.{}",
                        raw_cam_name.as_str(),
                        data.current_cam_settings_extension
                    );
                    let fullpath = cam_settings_path.clone().join(fname);
                    let mut fd = std::fs::File::create(&fullpath)?;
                    fd.write_all(data.current_cam_settings_buf.as_bytes())?;
                }
            }
        }

        // write feature detection settings
        {
            let mut feature_detect_settings_path = output_dirname.clone();
            feature_detect_settings_path.push(FEATURE_DETECT_SETTINGS_DIRNAME);
            if per_cam_data
                .iter()
                .any(|(_, x)| x.feature_detect_settings.is_some())
            {
                std::fs::create_dir_all(&feature_detect_settings_path)?;
            }

            for (raw_cam_name, cam) in per_cam_data.iter() {
                if let Some(data) = &cam.feature_detect_settings {
                    let buf = toml::to_vec(&data.current_feature_detect_settings)?;
                    let fname = format!("{}.toml", raw_cam_name.as_str());
                    let fullpath = feature_detect_settings_path.clone().join(fname);
                    let mut fd = std::fs::File::create(&fullpath)?;
                    fd.write_all(&buf)?;
                }
            }
        }

        // write cam info (pairs of CamNum and cam name)
        {
            let mut csv_path = output_dirname.clone();
            csv_path.push(format!("{}.gz", braid_types::CAM_INFO_CSV_FNAME));
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write + Send> =
                Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            let mut cam_info_wtr = csv::Writer::from_writer(fd);
            for row in cam_info_rows.iter() {
                cam_info_wtr.serialize(row)?;
            }
        }

        // write calibration
        if let Some(ref recon) = recon {
            let mut cal_path = output_dirname.clone();
            cal_path.push(braid_types::CALIBRATION_XML_FNAME);
            let fd = std::fs::File::create(&cal_path)?;
            recon.to_flydra_xml(fd)?;
        }

        // open textlog and write initial message
        let textlog_wtr = {
            let local_datetime = chrono::Local::now();
            let mainbrain_timestamp = strand_datetime_conversion::datetime_to_f64(&local_datetime);
            let (tzname_str, tzname) = match iana_time_zone::get_timezone() {
                Ok(tzname) => ("time_tzname0", tzname),
                Err(_err) => {
                    tracing::debug!("Could not get timezone, using UTC offset instead.");
                    let offset = local_datetime.offset();
                    use chrono::offset::Offset;
                    let offset_secs = offset.fix().local_minus_utc();
                    ("UTC_offset_secs", format!("{}", offset_secs))
                }
            };

            let fps = match fps {
                Some(fps) => format!("{}", fps),
                None => "unknown".to_string(),
            };
            let version = "2.0.0";
            let message = format!(
                "MainBrain running at {fps} fps, (\
                flydra_version {version}, git_revision {git_revision}, {tzname_str} {tzname})",
            );

            let tps = TrackingParamsSaver {
                tracking_params: (*tracking_params).clone(),
                git_revision,
            };
            let message2 = serde_json::to_string(&tps)?;

            let textlog: Vec<TextlogRow> = vec![
                TextlogRow {
                    mainbrain_timestamp,
                    cam_id: "mainbrain".to_string(),
                    host_timestamp: mainbrain_timestamp,
                    message,
                },
                TextlogRow {
                    mainbrain_timestamp,
                    cam_id: "mainbrain".to_string(),
                    host_timestamp: mainbrain_timestamp,
                    message: message2,
                },
            ];

            // We do not stream this to .gz because we want to maximize chances
            // that it is completely flushed to disk even in event of a panic.
            let mut csv_path = output_dirname.clone();
            csv_path.push(braid_types::TEXTLOG_CSV_FNAME);
            let fd = std::fs::File::create(&csv_path)?;
            let mut textlog_wtr =
                csv::Writer::from_writer(Box::new(fd) as Box<dyn std::io::Write + Send>);
            for row in textlog.iter() {
                textlog_wtr.serialize(row)?;
            }
            // Flush to disk. In case braid crashes, at least we want to recover this info.
            textlog_wtr.flush()?;
            textlog_wtr
        };

        // kalman estimates
        let kalman_estimates_wtr = if let Some(ref _recon) = recon {
            let mut csv_path = output_dirname.clone();
            csv_path.push(format!("{}.gz", braid_types::KALMAN_ESTIMATES_CSV_FNAME));
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write + Send> =
                Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            Some(OrderingWriter::new(csv::Writer::from_writer(fd)))
        } else {
            None
        };

        let trigger_clock_info_wtr = {
            let mut csv_path = output_dirname.clone();
            csv_path.push(format!("{}.gz", braid_types::TRIGGER_CLOCK_INFO_CSV_FNAME));
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write + Send> =
                Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            csv::Writer::from_writer(fd)
        };

        let experiment_info_wtr = {
            // We do not stream this to .gz because we want to maximize chances
            // that it is completely flushed to disk even in event of a panic.
            let mut csv_path = output_dirname.clone();
            csv_path.push(braid_types::EXPERIMENT_INFO_CSV_FNAME);
            let fd = std::fs::File::create(&csv_path)?;
            csv::Writer::from_writer(Box::new(fd) as Box<dyn std::io::Write + Send>)
        };

        let data_assoc_wtr = if let Some(ref _recon) = recon {
            let mut csv_path = output_dirname.clone();
            csv_path.push(format!("{}.gz", braid_types::DATA_ASSOCIATE_CSV_FNAME));
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write + Send> =
                Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            Some(csv::Writer::from_writer(fd))
        } else {
            None
        };

        let data_2d_wtr = {
            let mut csv_path = output_dirname.clone();
            csv_path.push(format!("{}.gz", braid_types::DATA2D_DISTORTED_CSV_FNAME));
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write + Send> =
                Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            csv::Writer::from_writer(fd)
        };

        let writer_stats = if cfg.print_stats { Some((0, 0)) } else { None };

        let file_start_time = if let Some(local) = local {
            local.into()
        } else {
            std::time::SystemTime::now()
        };

        let (reconstruction_latency_usec, reproj_dist_pixels) = if cfg.save_performance_histograms {
            (
                Some(HistogramWritingState::default()),
                Some(HistogramWritingState::default()),
            )
        } else {
            (None, None)
        };

        Ok(Self {
            output_dirname,
            readme_fd,
            save_empty_data2d,
            kalman_estimates_wtr,
            data_assoc_wtr,
            data_2d_wtr,
            textlog_wtr,
            trigger_clock_info_wtr,
            experiment_info_wtr,
            writer_stats,
            file_start_time,
            reconstruction_latency_usec,
            reproj_dist_pixels,
            last_flush: std::time::Instant::now(),
        })
    }

    fn save_data_2d_distorted(&mut self, fdp: FrameDataAndPoints) -> Result<usize> {
        let data2d_distorted = fdp.into_save(self.save_empty_data2d);
        for row in data2d_distorted.iter() {
            self.data_2d_wtr.serialize(row)?;
        }
        Ok(data2d_distorted.len())
    }

    fn flush_all(&mut self) -> Result<()> {
        if let Some(ref mut kew) = self.kalman_estimates_wtr {
            kew.flush()?;
        }
        if let Some(ref mut daw) = self.data_assoc_wtr {
            daw.flush()?;
        }
        self.data_2d_wtr.flush()?;
        self.textlog_wtr.flush()?;
        self.trigger_clock_info_wtr.flush()?;
        self.experiment_info_wtr.flush()?;
        self.last_flush = std::time::Instant::now();
        Ok(())
    }
}

impl Drop for WritingState {
    fn drop(&mut self) {
        tracing::debug!("WritingState is being dropped, flushing all data to disk.");
        fn dummy_csv() -> csv::Writer<Box<dyn std::io::Write + Send>> {
            let fd = Box::new(Vec::with_capacity(0));
            csv::Writer::from_writer(fd)
        }

        if let Some(count) = self.writer_stats {
            info!(
                "    {} rows of 2d detections, {} rows of kalman estimates",
                count.0, count.1
            );
        }

        // Drop all CSV files, which closes them.
        {
            self.kalman_estimates_wtr.take();
            self.data_assoc_wtr.take();
            // Could equivalently call `.flush()` on the writers?
            self.data_2d_wtr = dummy_csv();
            self.textlog_wtr = dummy_csv();
            self.trigger_clock_info_wtr = dummy_csv();
            self.experiment_info_wtr = dummy_csv();
        }

        // Move out original output name so that a subsequent call to `drop()`
        // doesn't accidentally overwrite our real data.
        let output_dirname = std::mem::take(&mut self.output_dirname);

        let now_system = std::time::SystemTime::now();
        {
            if let Some(reconstruction_latency_usec) = &mut self.reconstruction_latency_usec {
                finish_histogram(
                    &mut reconstruction_latency_usec.current_store,
                    self.file_start_time,
                    &mut reconstruction_latency_usec.histograms,
                    now_system,
                )
                .unwrap();

                save_hlog(
                    &output_dirname,
                    RECONSTRUCT_LATENCY_HLOG_FNAME,
                    &reconstruction_latency_usec.histograms,
                    self.file_start_time,
                );
            }

            if let Some(reproj_dist_pixels) = &mut self.reproj_dist_pixels {
                finish_histogram(
                    &mut reproj_dist_pixels.current_store,
                    self.file_start_time,
                    &mut reproj_dist_pixels.histograms,
                    now_system,
                )
                .unwrap();

                save_hlog(
                    &output_dirname,
                    REPROJECTION_DIST_HLOG_FNAME,
                    &reproj_dist_pixels.histograms,
                    self.file_start_time,
                );
            }
        }

        // Compress the saved directory into a .braidz file.
        {
            // TODO: read all the (forward) kalman estimates and smooth them to
            // an additional file. If we do it here, it is done after the
            // realtime tracking and thus does not interfere with recording
            // data. On the other hand, if we smooth at the end of each
            // trajectory, those smoothing costs are amortized throughout the
            // experiment.

            let replace_extension = match output_dirname.extension() {
                Some(ext) => ext == "braid",
                None => false,
            };

            // compute the name of the zip file.
            let output_zipfile: std::path::PathBuf = if replace_extension {
                output_dirname.with_extension("braidz")
            } else {
                let mut tmp = output_dirname.clone().into_os_string();
                tmp.push(".braidz");
                tmp.into()
            };

            info!("creating zip file {}", output_zipfile.display());
            braidz_writer::dir_to_braidz(&output_dirname, output_zipfile).unwrap();

            // Release the file so we no longer have exclusive access to the
            // directory. (Until we remove the directory, we have a small race
            // condition where another process could open the directory without
            // obtaining the readme file handle.)
            self.readme_fd = None;

            // Once the original directory is written successfully to a zip
            // file, we remove it.
            info!(
                "done creating zip file, removing {}",
                output_dirname.display()
            );
            std::fs::remove_dir_all(&output_dirname).unwrap();
        }
        tracing::debug!("Done writing braidz data to disk.");
    }
}

/// Listen to a Receiver for messages and save the data to disk.
///
/// This function only exits upon error or when the Sender counterpart to the
/// Receiver has closed. It blocks and does not use an async context and thus
/// should be spawned with `tokio::task::spawn_blocking`.
#[tracing::instrument(level = "debug", skip_all)]
pub(crate) fn writer_task_main(
    mut braidz_write_rx: tokio::sync::mpsc::Receiver<SaveToDiskMsg>,
    cam_manager: ConnectedCamerasManager,
    recon: Option<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
    tracking_params: Arc<TrackingParams>,
    save_empty_data2d: bool,
    metadata_builder: BraidMetadataBuilder,
    ignore_latency: bool,
) -> Result<()> {
    use crate::SaveToDiskMsg::*;
    use std::time::Duration;

    let mut writing_state: Option<WritingState> = None;

    const FLUSH_INTERVAL: u64 = 1;
    let flush_interval = Duration::from_secs(FLUSH_INTERVAL);

    tracing::debug!("Starting braidz writer task.");

    while let Some(msg) = braidz_write_rx.blocking_recv() {
        // TODO: improve flushing. Specifically, if we block for a long time
        // without receiving a message here, we will not flush to disk. To do
        // that, though, we would have a timeout on `blocking_recv`, which
        // doesn't seem possible.
        match msg {
            KalmanEstimate(ke) => {
                let KalmanEstimateRecord {
                    record,
                    data_assoc_rows,
                    mean_reproj_dist_100x,
                } = ke;
                let trigger_timestamp = record.timestamp.clone();

                // Now actually send the data to the writers.
                if let Some(ref mut ws) = writing_state {
                    if let Some(ref mut kew) = ws.kalman_estimates_wtr {
                        kew.serialize(record)?;
                        if let Some(count) = ws.writer_stats.as_mut() {
                            count.1 += 1
                        }
                    }
                    if let Some(ref mut daw) = ws.data_assoc_wtr {
                        for row in data_assoc_rows.iter() {
                            daw.serialize(row)?;
                        }
                    }

                    if !ignore_latency {
                        // Log reconstruction latency to histogram.
                        if let Some(trigger_timestamp) = trigger_timestamp {
                            // `trigger_timestamp` is when this frame was acquired.
                            // It may be None if it cannot be inferred while the
                            // triggerbox clock model is first initializing.
                            use chrono::{DateTime, Utc};
                            let then: DateTime<Utc> = trigger_timestamp.into();
                            let now = Utc::now();
                            let elapsed = now.signed_duration_since(then);
                            let now_system: std::time::SystemTime = now.into();

                            if let Some(latency_usec) = elapsed.num_microseconds() {
                                if latency_usec >= 0 {
                                    if let Some(reconstruction_latency_usec) =
                                        &mut ws.reconstruction_latency_usec
                                    {
                                        // The latency should always be positive, but num_microseconds()
                                        // can return negative and we don't want to panic if time goes
                                        // backwards for some reason.
                                        match histogram_record(
                                            latency_usec as u64,
                                            &mut reconstruction_latency_usec.current_store,
                                            1000 * 1000 * 60,
                                            2,
                                            ws.file_start_time,
                                            &mut reconstruction_latency_usec.histograms,
                                            now_system,
                                        ) {
                                            Ok(()) => {}
                                            Err(_) => tracing::error!(
                                                "latency value {} out of expected range",
                                                latency_usec
                                            ),
                                        }
                                    }
                                }
                            }
                        }
                    }

                    {
                        if let Some(mean_reproj_dist_100x) = mean_reproj_dist_100x {
                            let now_system = std::time::SystemTime::now();

                            if let Some(reproj_dist_pixels) = &mut ws.reproj_dist_pixels {
                                match histogram_record(
                                            mean_reproj_dist_100x,
                                            &mut reproj_dist_pixels.current_store,
                                            1000000,
                                            2,
                                            ws.file_start_time,
                                            &mut reproj_dist_pixels.histograms,
                                            now_system,
                                        ) {
                                            Ok(()) => {}
                                            Err(_) => tracing::error!(
                                                "mean reprojection 100x distance value {} out of expected range",
                                                mean_reproj_dist_100x
                                            ),
                                        }
                            }
                        }
                    }
                }

                // simply drop data if no file opened
            }
            Data2dDistorted(fdp) => {
                if let Some(ref mut ws) = writing_state {
                    let rows = ws.save_data_2d_distorted(fdp)?;
                    if let Some(count) = ws.writer_stats.as_mut() {
                        count.0 += rows;
                    }
                }
                // simply drop data if no file opened
            }
            StartSavingCsv(cfg) => {
                writing_state = Some(WritingState::new(
                    cfg,
                    cam_manager.sample(),
                    &recon,
                    tracking_params.clone(),
                    save_empty_data2d,
                    metadata_builder.clone(),
                )?);
            }
            StopSavingCsv => {
                // This will drop `writing_state`, and thus the writers, and
                // thus close them.
                writing_state = None;
            }
            SetExperimentUuid(uuid) => {
                let entry = ExperimentInfoRow { uuid };
                if let Some(ref mut ws) = writing_state {
                    ws.experiment_info_wtr.serialize(&entry)?;
                }
            }
            Textlog(entry) => {
                if let Some(ref mut ws) = writing_state {
                    ws.textlog_wtr.serialize(&entry)?;
                }
                // simply drop data if no file opened
            }
            TriggerClockInfo(entry) => {
                if let Some(ref mut ws) = writing_state {
                    ws.trigger_clock_info_wtr.serialize(&entry)?;
                }
                // simply drop data if no file opened
            }
        }

        if let Some(ref mut ws) = writing_state {
            if ws.last_flush.elapsed() > flush_interval {
                ws.flush_all()?;
            }
        }
    }
    tracing::info!("Done with braidz writer task.");
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn test_save_braidz_on_drop() {
        // create temporary dir to hold everything here.
        let root = tempfile::tempdir().unwrap().keep(); // must manually cleanup

        let braid_root = root.join("test.braid");
        let braidz_name = root.join("test.braidz");

        {
            let cfg = StartSavingCsvConfig {
                out_dir: braid_root.clone(),
                local: None,
                git_rev: "<impossible git rev>".into(),
                fps: None,
                per_cam_data: Default::default(),
                print_stats: false,
                save_performance_histograms: false,
            };

            let cam_manager = ConnectedCamerasManager::new(
                &None,
                std::collections::BTreeSet::new(),
                Arc::new(AtomicBool::new(true)),
                Arc::new(AtomicBool::new(true)),
                None,
            );
            let tracking_params = Arc::new(braid_types::default_tracking_params_full_3d());
            let save_empty_data2d = false;

            let ws = WritingState::new(
                cfg,
                cam_manager.sample(),
                &None,
                tracking_params,
                save_empty_data2d,
                BraidMetadataBuilder::saving_program_name(format!("{}:{}", file!(), line!())),
            )
            .unwrap();

            // Check that original directory exists.
            assert!(braid_root.exists());
            // Ensure .braidz not present.
            assert!(!braidz_name.exists());

            std::mem::drop(ws);
        }

        // Check that original directory is gone.
        assert!(!braid_root.exists());

        // Check that .braidz is present.
        assert!(braidz_name.exists());

        std::fs::remove_dir_all(root).unwrap();
    }

    /// Ensure that .braidz files can exceed 4GB.
    #[ignore]
    #[test]
    fn test_giant_braidz_for_zip64_support() -> Result<()> {
        let root = tempfile::tempdir()?;

        println!("saving giant files in temp dir {}", root.path().display());

        let braid_root = root.path().join("test.braid");
        let braidz_name = root.path().join("test.braidz");

        fn make_frame_data(i: u64) -> FrameDataAndPoints {
            let synced_frame = braid_types::SyncFno(i);
            FrameDataAndPoints {
                frame_data: crate::FrameData {
                    block_id: None,
                    cam_name: braid_types::RawCamName::new("cam".to_string()),
                    cam_num: braid_types::CamNum(0),
                    cam_received_timestamp: braid_types::FlydraFloatTimestampLocal::from_f64(
                        i as f64 + 0.123,
                    ),
                    device_timestamp: None,
                    synced_frame,
                    tdpt: crate::TimeDataPassthrough {
                        frame: synced_frame,
                        timestamp: None,
                    },
                    time_delta: crate::SyncedFrameCount {
                        frame: synced_frame,
                    },
                    trigger_timestamp: None,
                },
                points: vec![],
            }
        }

        // At 4.5 bytes per row, this gets us above 5_000_000_000 bytes.
        let num_rows = 1_200_000_000;

        let save_empty_data2d = true;
        {
            let cfg = StartSavingCsvConfig {
                out_dir: braid_root.clone(),
                local: None,
                git_rev: "<impossible git rev>".into(),
                fps: None,
                per_cam_data: Default::default(),
                print_stats: false,
                save_performance_histograms: false,
            };

            let cam_manager = ConnectedCamerasManager::new(
                &None,
                std::collections::BTreeSet::new(),
                Arc::new(AtomicBool::new(true)),
                Arc::new(AtomicBool::new(true)),
                None,
            );
            let tracking_params = Arc::new(braid_types::default_tracking_params_full_3d());

            let mut ws = WritingState::new(
                cfg,
                cam_manager.sample(),
                &None,
                tracking_params,
                save_empty_data2d,
                BraidMetadataBuilder::saving_program_name(format!("{}:{}", file!(), line!())),
            )?;

            // Check that original directory exists.
            assert!(braid_root.exists());
            // Ensure .braidz not present.
            assert!(!braidz_name.exists());

            // Save a lot of data
            for i in 0..num_rows {
                if i % 10_000_000 == 0 {
                    println!(
                        "writing {}/{}: {}%",
                        i,
                        num_rows,
                        i as f64 / num_rows as f64 * 100.0
                    );
                }
                ws.save_data_2d_distorted(make_frame_data(i))?;
            }

            std::mem::drop(ws);
        }

        // Check that original directory is gone.
        assert!(!braid_root.exists());

        // Check that .braidz is present.
        assert!(braidz_name.exists());

        let metadata = std::fs::metadata(&braidz_name)?;
        println!("metadata.len() {}", metadata.len());
        assert!(metadata.len() > 5_000_000_000);

        let zip_reader = std::fs::File::open(braidz_name)?;
        let mut zip_archive = zip::ZipArchive::new(zip_reader).unwrap();

        let data2d_fname = format!("{}.gz", braid_types::DATA2D_DISTORTED_CSV_FNAME);

        let gz_rdr = zip_archive.by_name(&data2d_fname).unwrap();

        let raw_csv_rdr = libflate::gzip::Decoder::new(gz_rdr)?;
        let csv_rdr = csv::Reader::from_reader(raw_csv_rdr);
        let csv_rdr2 = csv_rdr.into_deserialize();

        let mut count = 0;
        for (i, row) in csv_rdr2.into_iter().enumerate() {
            if i % 10_000_000 == 0 {
                println!(
                    "reading {}/{}: {}%",
                    i,
                    num_rows,
                    i as f64 / num_rows as f64 * 100.0
                );
            }
            let actual: braid_types::Data2dDistortedRow = row?;
            let mut expected_rows = make_frame_data(i as u64).into_save(save_empty_data2d);
            assert_eq!(expected_rows.len(), 1);
            let expected = expected_rows.pop().unwrap();
            let actual: braid_types::Data2dDistortedRowF32 = actual.into();
            assert_eq!(actual.frame, expected.frame);
            count += 1;
        }

        assert_eq!(count, num_rows);

        Ok(())
    }
}
