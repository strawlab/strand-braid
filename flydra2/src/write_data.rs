use crate::*;
use log::info;

use std::{io::Write, sync::Arc};

use flydra_types::{BRAID_SCHEMA, CAM_SETTINGS_DIRNAME, IMAGES_DIRNAME};

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
    data_assoc_wtr: Option<csv::Writer<Box<dyn std::io::Write>>>,
    data_2d_wtr: csv::Writer<Box<dyn std::io::Write>>,
    textlog_wtr: csv::Writer<Box<dyn std::io::Write>>,
    trigger_clock_info_wtr: csv::Writer<Box<dyn std::io::Write>>,
    experiment_info_wtr: csv::Writer<Box<dyn std::io::Write>>,
    writer_stats: Option<usize>,
    file_start_time: std::time::SystemTime,

    reconstruction_latency_usec: Option<HistogramWritingState>,
    reproj_dist_pixels: Option<HistogramWritingState>,
}

impl WritingState {
    fn new(
        cfg: StartSavingCsvConfig,
        cam_info_rows: Vec<CamInfoRow>,
        recon: &Option<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
        mut tracking_params: Arc<SwitchingTrackingParams>,
        save_empty_data2d: bool,
        saving_program_name: String,
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
            let readme_path = output_dirname.join(flydra_types::README_MD_FNAME);

            let mut fd = std::fs::File::create(&readme_path)?;

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
            let braid_metadata_path = output_dirname.join(flydra_types::BRAID_METADATA_YML_FNAME);

            let metadata = BraidMetadata {
                schema: BRAID_SCHEMA, // BraidMetadataSchemaTag
                git_revision: git_revision.clone(),
                original_recording_time: local,
                save_empty_data2d,
                saving_program_name,
            };
            let metadata_buf = serde_yaml::to_string(&metadata).unwrap();

            let mut fd = std::fs::File::create(&braid_metadata_path)?;
            fd.write_all(metadata_buf.as_bytes()).unwrap();
        }

        // write images
        {
            let mut image_path = output_dirname.clone();
            image_path.push(IMAGES_DIRNAME);
            std::fs::create_dir_all(&image_path)?;

            for cam in per_cam_data.iter() {
                if let Some(ref buf) = &cam.current_image_png {
                    let fname = format!("{}.png", cam.ros_cam_name.as_str());
                    let fullpath = image_path.clone().join(fname);
                    let mut fd = std::fs::File::create(&fullpath)?;
                    fd.write_all(buf)?;
                }
            }
        }

        // write camera settings
        {
            let mut settings_path = output_dirname.clone();
            settings_path.push(CAM_SETTINGS_DIRNAME);
            std::fs::create_dir_all(&settings_path)?;

            for cam in per_cam_data.iter() {
                if let Some(sd) = &cam.settings_data {
                    let fname = format!("{}.{}", cam.ros_cam_name.as_str(), sd.settings_file_ext);
                    let fullpath = settings_path.clone().join(fname);
                    let mut fd = std::fs::File::create(&fullpath)?;
                    fd.write_all(sd.settings_on_start.as_bytes())?;
                }
            }
        }

        // write cam info (pairs of CamNum and cam name)
        {
            let mut csv_path = output_dirname.clone();
            csv_path.push(format!("{}.gz", flydra_types::CAM_INFO_CSV_FNAME));
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write> = Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            let mut cam_info_wtr = csv::Writer::from_writer(fd);
            for row in cam_info_rows.iter() {
                cam_info_wtr.serialize(row)?;
            }
        }

        // write calibration
        if let Some(ref recon) = recon {
            let mut cal_path = output_dirname.clone();
            cal_path.push(flydra_types::CALIBRATION_XML_FNAME);
            let fd = std::fs::File::create(&cal_path)?;
            recon.to_flydra_xml(fd)?;
        }

        // open textlog and write initial message
        let textlog_wtr = {
            let timestamp = datetime_conversion::datetime_to_f64(&chrono::Local::now());

            let fps = match fps {
                Some(fps) => format!("{}", fps),
                None => "unknown".to_string(),
            };
            let version = "2.0.0";
            let tzname = iana_time_zone::get_timezone()?;
            let message = format!(
                "MainBrain running at {} fps, (\
                flydra_version {}, git_revision {}, time_tzname0 {})",
                fps, version, git_revision, tzname
            );

            let tps = TrackingParamsSaver {
                tracking_params: Arc::make_mut(&mut tracking_params).clone().into(), // convert to flydra_types::TrackingParams
                git_revision,
            };
            let message2 = serde_json::to_string(&tps)?;

            let textlog: Vec<TextlogRow> = vec![
                TextlogRow {
                    mainbrain_timestamp: timestamp,
                    cam_id: "mainbrain".to_string(),
                    host_timestamp: timestamp,
                    message,
                },
                TextlogRow {
                    mainbrain_timestamp: timestamp,
                    cam_id: "mainbrain".to_string(),
                    host_timestamp: timestamp,
                    message: message2,
                },
            ];

            // We do not stream this to .gz because we want to maximize chances
            // that it is completely flushed to disk even in event of a panic.
            let mut csv_path = output_dirname.clone();
            csv_path.push(flydra_types::TEXTLOG_CSV_FNAME);
            let fd = std::fs::File::create(&csv_path)?;
            let mut textlog_wtr = csv::Writer::from_writer(Box::new(fd) as Box<dyn std::io::Write>);
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
            csv_path.push(format!("{}.gz", flydra_types::KALMAN_ESTIMATES_CSV_FNAME));
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write> = Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            Some(OrderingWriter::new(csv::Writer::from_writer(fd)))
        } else {
            None
        };

        let trigger_clock_info_wtr = {
            let mut csv_path = output_dirname.clone();
            csv_path.push(format!("{}.gz", flydra_types::TRIGGER_CLOCK_INFO_CSV_FNAME));
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write> = Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            csv::Writer::from_writer(fd)
        };

        let experiment_info_wtr = {
            // We do not stream this to .gz because we want to maximize chances
            // that it is completely flushed to disk even in event of a panic.
            let mut csv_path = output_dirname.clone();
            csv_path.push(flydra_types::EXPERIMENT_INFO_CSV_FNAME);
            let fd = std::fs::File::create(&csv_path)?;
            csv::Writer::from_writer(Box::new(fd) as Box<dyn std::io::Write>)
        };

        let data_assoc_wtr = if let Some(ref _recon) = recon {
            let mut csv_path = output_dirname.clone();
            csv_path.push(format!("{}.gz", flydra_types::DATA_ASSOCIATE_CSV_FNAME));
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write> = Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            Some(csv::Writer::from_writer(fd))
        } else {
            None
        };

        let data_2d_wtr = {
            let mut csv_path = output_dirname.clone();
            csv_path.push(format!("{}.gz", flydra_types::DATA2D_DISTORTED_CSV_FNAME));
            let fd = std::fs::File::create(&csv_path)?;
            let fd: Box<dyn std::io::Write> = Box::new(AutoFinishUnchecked::new(Encoder::new(fd)?));
            csv::Writer::from_writer(fd)
        };

        let writer_stats = if cfg.print_stats { Some(0) } else { None };

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
        })
    }

    fn save_data_2d_distorted(&mut self, fdp: FrameDataAndPoints) -> Result<()> {
        let frame_data = &fdp.frame_data;
        let pts_to_save: Vec<Data2dDistortedRowF32> = fdp
            .points
            .iter()
            .map(|orig| convert_to_save(frame_data, orig))
            .collect();

        let data2d_distorted: Vec<Data2dDistortedRowF32> = if !pts_to_save.is_empty() {
            pts_to_save
        } else if self.save_empty_data2d {
            let empty_data = vec![convert_empty_to_save(frame_data)];
            empty_data
        } else {
            vec![]
        };

        for row in data2d_distorted.iter() {
            self.data_2d_wtr.serialize(&row)?;
        }
        Ok(())
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
        Ok(())
    }
}

impl Drop for WritingState {
    fn drop(&mut self) {
        fn dummy_csv() -> csv::Writer<Box<dyn std::io::Write>> {
            let fd = Box::new(Vec::with_capacity(0));
            csv::Writer::from_writer(fd)
        }

        if let Some(count) = self.writer_stats {
            info!("    {} rows of kalman estimates", count);
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
                    &mut reconstruction_latency_usec.histograms,
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
                    &mut reproj_dist_pixels.histograms,
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
            // zip the output_dirname directory
            {
                let mut file = std::fs::File::create(&output_zipfile).unwrap();

                let header = "BRAIDZ file. This is a standard ZIP file with a \
                    specific schema. You can view the contents of this \
                    file at https://braidz.strawlab.org/\n";
                file.write_all(header.as_bytes()).unwrap();

                let walkdir = walkdir::WalkDir::new(&output_dirname);

                // Reorder the results to save the README_MD_FNAME file first
                // so that the first bytes of the file have it. This is why we
                // special-case the file here.
                let mut readme_entry: Option<walkdir::DirEntry> = None;

                let mut files = Vec::new();
                for entry in walkdir.into_iter().filter_map(|e| e.ok()) {
                    if entry.file_name() == flydra_types::README_MD_FNAME {
                        readme_entry = Some(entry);
                    } else {
                        files.push(entry);
                    }
                }
                if let Some(entry) = readme_entry {
                    files.insert(0, entry);
                }

                let mut zipw = zip::ZipWriter::new(file);
                // Since most of our files are already compressed as .gz files,
                // we do not bother attempting to compress again. This would
                // cost significant computation but wouldn't save much space.
                // (The compressed files should all end with .gz so we could
                // theoretically compress the uncompressed files by a simple
                // file name filter. However, the README.md file should ideally
                // remain uncompressed and as the first file so that inspecting
                // the braidz file will show this.)
                let options = zip::write::FileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .large_file(true)
                    .unix_permissions(0o755);

                zip_dir::zip_dir(&mut files.into_iter(), &output_dirname, &mut zipw, options)
                    .expect("zip_dir");
                zipw.finish().unwrap();
            }

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
    }
}

pub(crate) fn writer_thread_main(
    save_data_rx: channellib::Receiver<SaveToDiskMsg>,
    cam_manager: ConnectedCamerasManager,
    recon: Option<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
    tracking_params: Arc<SwitchingTrackingParams>,
    save_empty_data2d: bool,
    saving_program_name: &str,
    ignore_latency: bool,
) -> Result<()> {
    use crate::SaveToDiskMsg::*;
    use std::time::{Duration, Instant};

    let mut writing_state: Option<WritingState> = None;

    const FLUSH_INTERVAL: u64 = 1;
    let flush_interval = Duration::from_secs(FLUSH_INTERVAL);

    let mut last_flushed = Instant::now();

    // TODO: add a timeout on recv() so that we periodically flush even if we
    // received no message.

    loop {
        match save_data_rx.recv() {
            Ok(msg) => {
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
                                    *count += 1
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
                                                    Err(_) => log::error!(
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
                                            Err(_) => log::error!(
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
                            ws.save_data_2d_distorted(fdp)?;
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
                            saving_program_name.to_string(),
                        )?);
                    }
                    StopSavingCsv => {
                        // This will drop the writers and thus close them.
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
                    QuitNow => {
                        // We rely on `writing_state.drop()` to flush and close
                        // everything.
                        break;
                    }
                };
            }
            Err(e) => {
                let _: channellib::RecvError = e;
                // sender disconnected. we can quit too.
                break;
            }
        };

        // after processing message, check if we should flush data.
        if last_flushed.elapsed() > flush_interval {
            // flush all writers
            if let Some(ref mut ws) = writing_state {
                ws.flush_all()?;
            }

            last_flushed = Instant::now();
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::{atomic::AtomicBool, Arc};

    #[test]
    fn test_save_braidz_on_drop() {
        // create temporary dir to hold everything here.
        let root = tempfile::tempdir().unwrap().into_path(); // must manually cleanup

        let braid_root = root.join("test.braid");
        let braidz_name = root.join("test.braidz");

        {
            let cfg = StartSavingCsvConfig {
                out_dir: braid_root.clone(),
                local: None,
                git_rev: "<impossible git rev>".into(),
                fps: None,
                per_cam_data: Vec::new(),
                print_stats: false,
                save_performance_histograms: false,
            };

            let cam_manager = ConnectedCamerasManager::new(
                &None,
                std::collections::BTreeSet::new(),
                Arc::new(AtomicBool::new(true)),
                Arc::new(AtomicBool::new(true)),
            );
            let tracking_params = Arc::new(SwitchingTrackingParams::default());
            let save_empty_data2d = false;

            let ws = WritingState::new(
                cfg,
                cam_manager.sample(),
                &None,
                tracking_params,
                save_empty_data2d,
                format!("{}:{}", file!(), line!()),
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
}
