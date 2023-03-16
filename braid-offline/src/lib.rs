#![cfg_attr(
    feature = "backtrace",
    feature(error_generic_member_access, provide_any)
)]

use log::{debug, info, warn};

use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
};

use flydra_types::CamInfoRow;

use braidz_parser::open_maybe_gzipped;

use flydra2::{
    CoordProcessor, CoordProcessorConfig, Data2dDistortedRow, FrameData, FrameDataAndPoints,
    NumberedRawUdpPoint, StreamItem,
};
use groupby::{AscendingGroupIter, BufferedSortIter};

use flydra_types::{
    PerCamSaveData, RawCamName, RosCamName, SyncFno, TrackingParams,
    FEATURE_DETECT_SETTINGS_DIRNAME, IMAGES_DIRNAME,
};

#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{source}")]
    Io {
        #[from]
        source: std::io::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Flydra2 {
        #[from]
        source: flydra2::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    BraidzParser {
        #[from]
        source: braidz_parser::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("output filename must end with '.braidz'")]
    OutputFilenameMustEndInBraidz {
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("No calibration found")]
    NoCalibrationFound,
    #[error("{source}")]
    ZipDir {
        #[from]
        source: zip_or_dir::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    FuturesSendError {
        #[from]
        source: futures::channel::mpsc::SendError,
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
}

fn to_point_info(row: &Data2dDistortedRow, idx: u8) -> NumberedRawUdpPoint {
    let maybe_slope_eccentricty = if row.area.is_nan() {
        None
    } else {
        Some((row.slope, row.eccentricity))
    };
    NumberedRawUdpPoint {
        idx,
        pt: flydra_types::FlydraRawUdpPoint {
            x0_abs: row.x,
            y0_abs: row.y,
            area: row.area,
            maybe_slope_eccentricty,
            cur_val: row.cur_val,
            mean_val: row.mean_val,
            sumsqf_val: row.sumsqf_val,
        },
    }
}

fn safe_u64(val: i64) -> u64 {
    if val < 0 {
        panic!("value out of range");
    }
    val as u64
}

fn split_by_cam(invec: Vec<Data2dDistortedRow>) -> Vec<Vec<Data2dDistortedRow>> {
    let mut by_cam = BTreeMap::new();

    for inrow in invec.into_iter() {
        let rows_entry = &mut by_cam.entry(inrow.camn).or_insert_with(Vec::new);
        rows_entry.push(inrow);
    }

    by_cam.into_iter().map(|(_k, v)| v).collect()
}

// TODO fix DRY with incremental_parser
fn calc_fps_from_data<R: Read>(data_file: R) -> flydra2::Result<f64> {
    let rdr = csv::Reader::from_reader(data_file);
    let mut data_iter = rdr.into_deserialize();
    let row0: Option<std::result::Result<Data2dDistortedRow, _>> = data_iter.next();
    if let Some(Ok(row0)) = row0 {
        let mut last_row = None;
        for row in data_iter {
            last_row = match row {
                Ok(row) => Some(row),
                Err(e) => {
                    log::error!("error reading 2d data when calculating fps: {} {:?}", e, e);
                    continue;
                }
            };
        }
        if let Some(last_row) = last_row {
            debug!(
                "2d data: Start frame {}, end frame {}. {}:{}",
                row0.frame,
                last_row.frame,
                file!(),
                line!()
            );
            let df = last_row.frame - row0.frame;
            if last_row.timestamp.is_some() && row0.timestamp.is_some() {
                // timestamp from trigger-derived source (should be more accurate)
                let ts1 = last_row.timestamp.map(|x| x.as_f64()).unwrap();
                let ts0 = row0.timestamp.map(|x| x.as_f64()).unwrap();
                let dt = ts1 - ts0;
                Ok(df as f64 / dt)
            } else {
                // timestamp from host clock (should always be present)
                let dt =
                    last_row.cam_received_timestamp.as_f64() - row0.cam_received_timestamp.as_f64();
                Ok(df as f64 / dt)
            }
        } else {
            debug!(
                "2d data: Single frame {}. {}:{}",
                row0.frame,
                file!(),
                line!()
            );
            Err(flydra2::Error::InsufficientDataToCalculateFps)
        }
    } else {
        debug!("no 2d data could be read. {}:{}", file!(), line!());
        Err(flydra2::Error::InsufficientDataToCalculateFps)
    }
}

#[derive(Debug, Clone)]
pub struct KalmanizeOptions {
    pub start_frame: Option<u64>,
    pub stop_frame: Option<u64>,
    pub model_server_addr: Option<String>,
}

impl Default for KalmanizeOptions {
    fn default() -> Self {
        Self {
            start_frame: None,
            stop_frame: None,
            model_server_addr: None,
        }
    }
}

/// Perform offline tracking on the data
///
/// - `data_src` is the input data. It must be a `.braidz` file (or a `.braid`
///   directory). Create with `convert_strand_cam_csv_to_flydra_csv_dir` or
///   `convert_flydra1_mainbrain_h5_to_csvdir`.
/// - `output_braidz` is the final .braidz file into which the resulting files
///   will be saved. Upon closing all files, this is typically zipped and saved
///   to .braidz file.
///
/// Note that a temporary directly ending with `.braid` is initially created and
/// only on upon completed tracking is this converted to the output .braidz
/// file.
pub async fn kalmanize<Q, R>(
    mut data_src: braidz_parser::incremental_parser::IncrementalParser<
        R,
        braidz_parser::incremental_parser::BasicInfoParsed,
    >,
    output_braidz: Q,
    expected_fps: Option<f64>,
    tracking_params: TrackingParams,
    opt2: KalmanizeOptions,
    rt_handle: tokio::runtime::Handle,
    save_performance_histograms: bool,
    saving_program_name: &str,
) -> Result<(), Error>
where
    Q: AsRef<Path>,
    R: 'static + Read + Seek + Send,
{
    let output_braidz = output_braidz.as_ref();
    let output_dirname = if output_braidz.extension() == Some(std::ffi::OsStr::new("braidz")) {
        let mut output_dirname: PathBuf = output_braidz.to_path_buf();
        output_dirname.set_extension("braid");
        output_dirname
    } else {
        return Err(Error::OutputFilenameMustEndInBraidz {
            #[cfg(feature = "backtrace")]
            backtrace: std::backtrace::Backtrace::capture(),
        });
    };

    info!("tracking:");
    info!("  {} -> {}", data_src.display(), output_dirname.display());

    let src_info = data_src.basic_info();

    let recon = if let Some(ci) = &src_info.calibration_info {
        let cams = ci.cameras.clone();
        let water = ci.water;
        flydra_mvg::FlydraMultiCameraSystem::from_system(cams, water)
    } else {
        return Err(Error::NoCalibrationFound);
    };

    let all_expected_cameras = recon
        .cam_names()
        .map(|x| RosCamName::new(x.to_string()))
        .collect();

    let signal_all_cams_present = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let signal_all_cams_synced = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let mut cam_manager = flydra2::ConnectedCamerasManager::new(
        &Some(recon.clone()),
        all_expected_cameras,
        signal_all_cams_present,
        signal_all_cams_synced,
    );

    // Create `stream_cancel::Valve` for shutting everything down. Note this is
    // `Clone`, so we can (and should) shut down everything with it. Here we let
    // _quit_trigger drop when it goes out of scope. This is due to use in this
    // offline context.
    let (_quit_trigger, valve) = stream_cancel::Valve::new();

    let (frame_data_tx, frame_data_rx) = tokio::sync::mpsc::channel(10);
    let frame_data_rx = tokio_stream::wrappers::ReceiverStream::new(frame_data_rx);
    let save_empty_data2d = true;
    let ignore_latency = true;
    let mut coord_processor = CoordProcessor::new(
        CoordProcessorConfig {
            tracking_params,
            save_empty_data2d,
            ignore_latency,
        },
        rt_handle.clone(),
        cam_manager.clone(),
        Some(recon.clone()),
        saving_program_name,
        valve,
    )?;

    for cam_name in recon.cam_names() {
        let mut old_image_fname = data_src.path_starter();
        old_image_fname.push(IMAGES_DIRNAME);
        old_image_fname.push(cam_name);
        old_image_fname.set_extension("png");

        if !old_image_fname.exists() {
            warn!("Image file {} not found", old_image_fname.display());
            continue;
        }

        let mut new_image_fname: PathBuf = output_dirname.to_path_buf();
        new_image_fname.push(IMAGES_DIRNAME);
        std::fs::create_dir_all(&new_image_fname)?; // Create dir if needed.
        new_image_fname.push(&cam_name);
        new_image_fname.set_extension("png");

        let reader = old_image_fname.open()?;
        copy_to(reader, new_image_fname)?;
    }

    // open the data2d CSV file
    let mut data_fname = data_src.path_starter();
    data_fname.push(flydra_types::DATA2D_DISTORTED_CSV_FNAME);

    let fps = match expected_fps {
        Some(fps) => fps,
        None => {
            // TODO: replace with implementation in braidz-parser.
            let data_file = open_maybe_gzipped(data_fname)?;

            // TODO: first choice parse "MainBrain running at {}" string (as in
            // braidz-parser). Second choice, do this.
            calc_fps_from_data(data_file)?
        }
    };

    let images_dirname = data_src.path_starter().join(IMAGES_DIRNAME);

    let per_cam_data: BTreeMap<RosCamName, PerCamSaveData> = match images_dirname.list_paths() {
        Ok(relnames) => relnames
            .iter()
            .map(|relname| {
                assert_eq!(relname.extension(), Some(std::ffi::OsStr::new("png")));
                let ros_cam_name =
                    RosCamName::new(relname.file_stem().unwrap().to_str().unwrap().to_string());

                let png_fname = data_src.path_starter().join(IMAGES_DIRNAME).join(relname);
                let current_image_png = {
                    let mut fd = png_fname.open().unwrap();
                    let mut buf = vec![];
                    fd.read_to_end(&mut buf).unwrap();
                    buf
                };

                let mut current_feature_detect_settings_fname = data_src
                    .path_starter()
                    .join(FEATURE_DETECT_SETTINGS_DIRNAME)
                    .join(format!("{}.toml", ros_cam_name.as_str()));

                let current_feature_detect_settings =
                    if current_feature_detect_settings_fname.exists() {
                        let mut fd = current_feature_detect_settings_fname.open().unwrap();
                        let mut buf = vec![];
                        fd.read_to_end(&mut buf).unwrap();
                        toml::from_slice(&buf).unwrap()
                    } else {
                        flydra_pt_detect_cfg::default_absdiff()
                    };

                (
                    ros_cam_name,
                    PerCamSaveData {
                        current_image_png: current_image_png.into(),
                        cam_settings_data: None,
                        feature_detect_settings: Some(flydra_types::UpdateFeatureDetectSettings {
                            current_feature_detect_settings,
                        }),
                    },
                )
            })
            .collect(),
        Err(zip_or_dir::Error::NotDirectory(_)) => Default::default(),
        Err(e) => return Err(e.into()),
    };

    // read the cam_info CSV file
    let mut cam_info_fname = data_src.path_starter();
    cam_info_fname.push(flydra_types::CAM_INFO_CSV_FNAME);
    let cam_info_file = open_maybe_gzipped(cam_info_fname)?;
    let mut orig_camn_to_cam_name: BTreeMap<flydra_types::CamNum, RosCamName> = BTreeMap::new();
    let rdr = csv::Reader::from_reader(cam_info_file);
    for row in rdr.into_deserialize::<CamInfoRow>() {
        let row = row?;

        let orig_cam_name = RawCamName::new(row.cam_id.to_string());
        let ros_cam_name = RosCamName::new(row.cam_id.to_string());
        let no_server = flydra_types::CamHttpServerInfo::NoServer;

        orig_camn_to_cam_name.insert(row.camn, ros_cam_name.clone());

        cam_manager.register_new_camera(&orig_cam_name, &no_server, &ros_cam_name);
    }

    {
        let braidz_write_tx = coord_processor.get_braidz_write_tx();
        let save_cfg = flydra2::StartSavingCsvConfig {
            out_dir: output_dirname.to_path_buf(),
            local: None,
            git_rev: env!("GIT_HASH").to_string(),
            fps: Some(fps as f32),
            per_cam_data,
            print_stats: true,
            save_performance_histograms,
        };

        braidz_write_tx
            .send(flydra2::SaveToDiskMsg::StartSavingCsv(save_cfg))
            .await
            .unwrap();
        // It is important to drop the `braidz_write_tx` because it contains a
        // Sender to the writing task and if this is not dropped, the writing
        // task never completes.
    }

    let opt3 = opt2.clone();

    // Construct a local task set that can run `!Send` futures.
    // `open_maybe_gzipped` returns a non-Send result.
    let local = tokio::task::LocalSet::new();

    // Run the local task set.
    let reader_local_future = local.run_until(async move {
        // TODO: Consolidate this code with the `braidz-parser` crate. Right now
        // there is substantial duplication.

        // open the data2d CSV file
        let mut data_fname = data_src.path_starter();
        data_fname.push(flydra_types::DATA2D_DISTORTED_CSV_FNAME);

        log::trace!("loading data from {}", data_fname.display());

        let display_fname = format!("{}", data_fname.display());
        let data_file = open_maybe_gzipped(data_fname)?;
        let rdr = csv::Reader::from_reader(data_file);
        let data_iter = rdr.into_deserialize();

        let bufsize = 10000;
        let sorted_data_iter = BufferedSortIter::new(data_iter, bufsize)
            .map_err(|e| flydra2::file_error("reading rows", display_fname.clone(), e))?;
        // let rdr = sorted_data_iter.inner().reader();

        let data_row_frame_iter = AscendingGroupIter::new(sorted_data_iter);
        // let rdr = data_row_frame_iter.inner().inner().reader();

        for data_frame_rows in data_row_frame_iter {
            // let pos = rdr.position();

            // we are now in a loop where all rows come from the same frame, but not necessarily the same camera
            let data_frame_rows = data_frame_rows?;

            let rows = data_frame_rows.rows;
            let synced_frame = SyncFno(safe_u64(data_frame_rows.group_key));

            let opt = opt3.clone();
            if let Some(ref start) = &opt.start_frame {
                if synced_frame.0 < *start {
                    continue;
                }
            }

            if let Some(ref stop) = &opt.stop_frame {
                if synced_frame.0 > *stop {
                    break;
                }
            }

            for cam_rows in split_by_cam(rows).iter() {
                let cam_name = orig_camn_to_cam_name
                    .get(&cam_rows[0].camn)
                    .expect("camn missing")
                    .clone();
                let trigger_timestamp = cam_rows[0].timestamp.clone();
                let cam_received_timestamp = cam_rows[0].cam_received_timestamp.clone();
                let device_timestamp = cam_rows[0].device_timestamp.clone();
                let block_id = cam_rows[0].block_id.clone();
                let points = cam_rows
                    .iter()
                    .enumerate()
                    .map(|(i, p)| to_point_info(p, i as u8))
                    .collect();

                let cam_num = cam_manager.cam_num(&cam_name).unwrap();

                let frame_data = FrameData::new(
                    cam_name,
                    cam_num,
                    synced_frame,
                    trigger_timestamp,
                    cam_received_timestamp,
                    device_timestamp,
                    block_id,
                );
                let fdp = FrameDataAndPoints { frame_data, points };
                // block until sent
                match frame_data_tx.send(StreamItem::Packet(fdp)).await {
                    Ok(()) => {}
                    Err(e) => {
                        log::error!("send error {} at {}:{}", e, file!(), line!())
                    }
                }
            }
        }

        match frame_data_tx.send(StreamItem::EOF).await {
            Ok(()) => {}
            Err(e) => {
                log::error!("send error {} at {}:{}", e, file!(), line!())
            }
        }

        Ok::<(), anyhow::Error>(())
    });

    let expected_framerate = Some(fps as f32);

    // let model_server_addr = opt.model_server_addr.clone();

    let (_quit_trigger, valve) = stream_cancel::Valve::new();
    let (data_tx, data_rx) = tokio::sync::mpsc::channel(50);

    let _model_server = match &opt2.model_server_addr {
        Some(ref addr) => {
            let addr = addr.parse().unwrap();
            info!("send_pose server at {}", addr);
            let info = flydra_types::StaticMainbrainInfo {
                name: env!("CARGO_PKG_NAME").into(),
                version: env!("CARGO_PKG_VERSION").into(),
            };
            coord_processor.add_listener(data_tx);
            Some(flydra2::new_model_server(data_rx, valve, None, &addr, info, rt_handle).await?)
        }
        None => None,
    };

    // TODO: reorder incoming CSV lines to be monotonic w.r.t. frames? This
    // would cause behavior to diverge from online system but would result in
    // better retracking. Perhaps solution is to give runtime option to do
    // either.

    let consume_future = coord_processor.consume_stream(frame_data_rx, expected_framerate);

    let (writer_jh, r2) = tokio::join!(consume_future, reader_local_future);

    writer_jh
        .await
        .expect("finish writer task 1")
        .expect("finish writer task 2");
    r2.expect("finish reader task");

    Ok(())
}

/// Copy from `reader` to path `dest`.
fn copy_to<R, P>(mut reader: R, dest: P) -> flydra2::Result<()>
where
    R: Read,
    P: AsRef<Path>,
{
    let mut buf = vec![];
    let mut new_file = File::create(dest)?;
    reader.read_to_end(&mut buf)?;
    new_file.write_all(&buf)?;
    new_file.flush()?;
    Ok(())
}

fn open_buffered<P: AsRef<Path>>(p: &P) -> std::io::Result<std::io::BufReader<File>> {
    Ok(std::io::BufReader::new(File::open(p.as_ref())?))
}

/// Load .csv or .csv.gz file.
///
/// This function should only be used in the `braid-offline` crate. This
/// function would ideally not be marked `pub` but due to visibility rules, it
/// must be marked `pub` do use it in the `compute-flydra1-compat` binary.
///
/// This should not be used in the general case but only for special cases where
/// a raw directory is being used, such as specifically when modifying a
/// directory under construction. For the general reading case, prefer
/// `braidz_parser` crate (or the `zip_or_dir` if it may not be a valid braidz
/// archive) crate.
pub fn pick_csvgz_or_csv(csv_path: &Path) -> flydra2::Result<Box<dyn Read>> {
    let gz_fname = PathBuf::from(csv_path).with_extension("csv.gz");

    if csv_path.exists() {
        open_buffered(&csv_path)
            .map(|fd| {
                let rdr: Box<dyn Read> = Box::new(fd); // type erasure
                rdr
            })
            .map_err(|e| {
                flydra2::file_error("opening", format!("opening {}", csv_path.display()), e)
            })
    } else {
        // This gives us an error corresponding to a non-existing .gz file.
        let gz_fd = open_buffered(&gz_fname).map_err(|e| {
            flydra2::file_error("opening", format!("opening {}", gz_fname.display()), e)
        })?;
        let decoder = libflate::gzip::Decoder::new(gz_fd)?;
        Ok(Box::new(decoder))
    }
}
