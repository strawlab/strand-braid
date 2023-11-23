#![cfg_attr(feature = "backtrace", feature(error_generic_member_access))]

use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
};

use anyhow::Context;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use ordered_float::NotNan;
use tracing::{debug, info, warn};

use braidz_parser::open_maybe_gzipped;
use flydra2::{
    CoordProcessor, CoordProcessorConfig, Data2dDistortedRow, FrameData, FrameDataAndPoints,
    NumberedRawUdpPoint, StreamItem,
};
use flydra_types::{
    CamInfoRow, PerCamSaveData, RawCamName, RosCamName, SyncFno, TrackingParams,
    FEATURE_DETECT_SETTINGS_DIRNAME, IMAGES_DIRNAME,
};
use groupby::{AscendingGroupIter, BufferedSortIter};

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
    val.try_into().unwrap()
}

fn split_by_cam(invec: Vec<Data2dDistortedRow>) -> Vec<Vec<Data2dDistortedRow>> {
    let mut by_cam = BTreeMap::new();

    for inrow in invec.into_iter() {
        let rows_entry = &mut by_cam.entry(inrow.camn).or_insert_with(Vec::new);
        rows_entry.push(inrow);
    }

    by_cam.into_values().collect()
}

#[tracing::instrument]
fn calc_fps_from_data<R: Read + std::fmt::Debug>(data_file: R) -> flydra2::Result<f64> {
    let rdr = csv::Reader::from_reader(data_file);
    let mut data_iter = rdr.into_deserialize();
    let row0: Option<std::result::Result<Data2dDistortedRow, _>> = data_iter.next();
    if let Some(Ok(row0)) = row0 {
        let mut last_row = None;
        for row in data_iter {
            last_row = match row {
                Ok(row) => Some(row),
                Err(e) => {
                    tracing::error!("error reading 2d data when calculating fps: {} {:?}", e, e);
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

// AscendingGroupIter<i64, BufferedSortIter<i64, DeserializeRecordsIntoIter<Box<dyn Read>, Data2dDistortedRow>, Data2dDistortedRow, Error>, Data2dDistortedRow, Error>

// fn my_open_file<'a, R: Read + Seek>(
//     mut data_fname: zip_or_dir::PathLike<'a, R>,
// ) -> Result<impl Iterator<Item = Vec<Data2dDistortedRow>>, Error> {

//     let display_fname = format!("{}", data_fname.display());

//     let data_file = open_maybe_gzipped(data_fname)?;
//     let rdr = csv::Reader::from_reader(data_file);
//     // let file_reader = rdr.get_ref();
//     // let file_size = file_reader.size();
//     let data_iter = rdr.into_deserialize();

//     let bufsize = 10000;
//     let sorted_data_iter = BufferedSortIter::new(data_iter, bufsize)
//         .map_err(|e| flydra2::file_error("reading rows", display_fname.clone(), e))?;
//     // let rdr = sorted_data_iter.inner().reader();

//     let data_row_frame_iter = AscendingGroupIter::new(sorted_data_iter);
//     Ok(data_row_frame_iter)
// }

#[derive(Debug, Clone, Default)]
pub struct KalmanizeOptions {
    pub start_frame: Option<u64>,
    pub stop_frame: Option<u64>,
    pub model_server_addr: Option<String>,
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
#[tracing::instrument]
#[allow(clippy::too_many_arguments)]
pub async fn kalmanize<Q, R>(
    mut data_src: braidz_parser::incremental_parser::IncrementalParser<
        R,
        braidz_parser::incremental_parser::BasicInfoParsed,
    >,
    output_braidz: Q,
    forced_fps: Option<NotNan<f64>>,
    tracking_params: TrackingParams,
    opt2: KalmanizeOptions,
    rt_handle: tokio::runtime::Handle,
    save_performance_histograms: bool,
    saving_program_name: &str,
    no_progress: bool,
) -> Result<(), Error>
where
    Q: AsRef<Path> + std::fmt::Debug,
    R: 'static + Read + Seek + Send + std::fmt::Debug,
{
    let mini_arena_debug_image_dir = output_braidz.as_ref().parent().map(PathBuf::from);
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

    info!(
        "tracking: {} -> {}",
        data_src.display(),
        output_dirname.display()
    );

    let metadata_builder = flydra2::BraidMetadataBuilder::saving_program_name(saving_program_name);

    let (local, metadata_fps, recon) = {
        let src_info = data_src.basic_info();
        let cam_ids: Vec<String> = src_info
            .cam_info
            .camid2camn
            .keys()
            .map(Clone::clone)
            .collect();
        let local = src_info.metadata.original_recording_time.clone();

        let recon = if let Some(ci) = &src_info.calibration_info {
            // Check if we need to convert "real" camera names to ROS-compatible
            // names. We are trying to move everywhere to "real" camera names,
            // but old code (and perhaps current code) converts the real names
            // to ROS-compatible names. E.g. real name "Basler-1234" ROS name
            // "Basler_1234".
            let mut cams = ci.cameras.clone();
            let mut found = 0;
            let mut count = 0;
            for cam_id_in_calibration in cams.cams_by_name().keys() {
                count += 1;
                info!("Calibration contains camera: {cam_id_in_calibration}");
                if !cam_ids.iter().any(|x| x == cam_id_in_calibration) {
                    let ros_name_calib = RawCamName::new(cam_id_in_calibration.clone()).to_ros();
                    if cam_ids
                        .iter()
                        .any(|x| x.as_str() == ros_name_calib.as_str())
                    {
                        found += 1;
                    }
                }
            }
            if found > 0 && found == count {
                info!("Converting camera calibration names from original to ROS-compatible names.");
                let mut new_cams = std::collections::BTreeMap::new();
                for (orig_name, orig_value) in cams.cams_by_name().iter() {
                    let ros_name = RawCamName::new(orig_name.clone())
                        .to_ros()
                        .as_str()
                        .to_string();
                    new_cams.insert(ros_name, orig_value.clone());
                }
                cams = if let Some(comment) = cams.comment() {
                    mvg::MultiCameraSystem::new_with_comment(new_cams, comment.clone())
                } else {
                    mvg::MultiCameraSystem::new(new_cams)
                };
            }
            let water = ci.water;
            flydra_mvg::FlydraMultiCameraSystem::from_system(cams, water)
        } else {
            return Err(Error::NoCalibrationFound);
        };

        (local, src_info.expected_fps, recon)
    };

    let fps = if let Some(fps) = forced_fps {
        fps.into_inner()
    } else {
        if !metadata_fps.is_nan() {
            metadata_fps
        } else {
            // FPS could not be determined from metadata. Read the data to determine it.
            let data_src_name = format!("{}", data_src.display());
            let data_fname = data_src
                .path_starter()
                .join(flydra_types::DATA2D_DISTORTED_CSV_FNAME);

            warn!(
                "File \"{}\" does not have FPS saved directly. Will \
                    parse from data.",
                data_src_name
            );

            // TODO: replace with implementation in braidz-parser.
            let data_file = open_maybe_gzipped(data_fname)?;

            // TODO: first choice parse "MainBrain running at {}" string (as in
            // braidz-parser). Second choice, do this.
            calc_fps_from_data(data_file)?
        }
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
            mini_arena_debug_image_dir,
        },
        rt_handle.clone(),
        cam_manager.clone(),
        Some(recon.clone()),
        metadata_builder.clone(),
        valve,
    )?;

    let images_dirname = data_src.path_starter().join(IMAGES_DIRNAME);
    let mut found_image_paths: Vec<_> = match images_dirname.list_paths() {
        Ok(paths) => paths,
        Err(zip_or_dir::Error::NotDirectory(_)) => vec![],
        Err(e) => {
            return Err(e.into());
        }
    };

    for cam_name in recon.cam_names() {
        let fname = format!("{}.png", cam_name);
        let fname = fname.as_str();

        let mut old_image_fname = data_src.path_starter();
        old_image_fname.push(IMAGES_DIRNAME);
        old_image_fname.push(fname);

        if !old_image_fname.exists() {
            warn!("Image file {} not found", old_image_fname.display());
            continue;
        }

        if let Some(idx) = found_image_paths
            .iter()
            .position(|x| format!("{}", x.display()) == *fname)
        {
            found_image_paths.remove(idx);
        }

        let mut new_image_fname: PathBuf = output_dirname.to_path_buf();
        new_image_fname.push(IMAGES_DIRNAME);
        std::fs::create_dir_all(&new_image_fname)?; // Create dir if needed.
        new_image_fname.push(cam_name);
        new_image_fname.set_extension("png");

        let reader = old_image_fname.open()?;
        copy_to(reader, new_image_fname)?;
    }

    for unused in found_image_paths.iter() {
        tracing::warn!(
            "Unexpected file {}/{} found",
            IMAGES_DIRNAME,
            unused.display()
        );
    }

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
            local,
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

        // OK, this is stupid - we parse the entire CSV file simply to determine
        // how many rows it has for our progress bar.
        let n_csv_frames = if no_progress {
            None
        } else {
            tracing::info!(
                "Parsing {} file to determine frame count.",
                flydra_types::DATA2D_DISTORTED_CSV_FNAME
            );
            // open the data2d CSV file
            let mut data_fname = data_src.path_starter();
            data_fname.push(flydra_types::DATA2D_DISTORTED_CSV_FNAME);

            tracing::trace!("loading data from {}", data_fname.display());

            let display_fname = format!("{}", data_fname.display());

            let data_file = open_maybe_gzipped(data_fname)?;
            let rdr = csv::Reader::from_reader(data_file);
            let data_iter = rdr.into_deserialize();

            let bufsize = 10000;
            let sorted_data_iter = BufferedSortIter::new(data_iter, bufsize)
                .map_err(|e| flydra2::file_error("reading rows", display_fname.clone(), e))?;

            let data_row_frame_iter = AscendingGroupIter::new(sorted_data_iter);
            let mut count = 0;
            let mut min_frame = std::u64::MAX;
            let mut max_frame = 0;
            for data_frame_rows in data_row_frame_iter {
                let data_frame_rows: groupby::GroupedRows<i64, Data2dDistortedRow> =
                    data_frame_rows?;
                if let Some(start_frame) = opt3.start_frame {
                    if safe_u64(data_frame_rows.group_key) < start_frame {
                        continue;
                    }
                }
                let this_frame = safe_u64(data_frame_rows.group_key);
                if let Some(stop_frame) = opt3.stop_frame {
                    if this_frame > stop_frame {
                        break;
                    }
                }
                if this_frame < min_frame {
                    min_frame = this_frame;
                }
                if this_frame > max_frame {
                    max_frame = this_frame;
                }
                count += 1;
            }
            tracing::info!("Will process {count} frames (Range: {min_frame} - {max_frame}).");
            Some(count)
        };

        let data_row_frame_iter = {
            // open the data2d CSV file
            let mut data_fname = data_src.path_starter();
            data_fname.push(flydra_types::DATA2D_DISTORTED_CSV_FNAME);

            tracing::trace!("loading data from {}", data_fname.display());

            let display_fname = format!("{}", data_fname.display());

            let data_file = open_maybe_gzipped(data_fname)?;
            let rdr = csv::Reader::from_reader(data_file);
            let data_iter = rdr.into_deserialize();

            let bufsize = 10000;
            let sorted_data_iter = BufferedSortIter::new(data_iter, bufsize)
                .map_err(|e| flydra2::file_error("reading rows", display_fname.clone(), e))?;
            let data_row_frame_iter = AscendingGroupIter::new(sorted_data_iter);
            data_row_frame_iter
        };

        let pb = if let Some(n_csv_frames) = n_csv_frames {
            // Custom progress bar with space at right end to prevent obscuring last
            // digit with cursor.
            let style = ProgressStyle::with_template("{wide_bar} {pos}/{len} ETA: {eta} ")?;
            Some(ProgressBar::new(n_csv_frames.try_into().unwrap()).with_style(style))
        } else {
            None
        };

        for data_frame_rows in data_row_frame_iter {
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

            if let Some(pb) = &pb {
                // Increment the counter.
                pb.inc(1);
            }

            for cam_rows in split_by_cam(rows).iter() {
                let cam_name = orig_camn_to_cam_name
                    .get(&cam_rows[0].camn)
                    .expect("camn missing")
                    .clone();
                let trigger_timestamp = cam_rows[0].timestamp.clone();
                let cam_received_timestamp = cam_rows[0].cam_received_timestamp.clone();
                let device_timestamp = cam_rows[0].device_timestamp;
                let block_id = cam_rows[0].block_id;
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
                        tracing::error!("send error {} at {}:{}", e, file!(), line!())
                    }
                }
            }
        }

        match frame_data_tx.send(StreamItem::EOF).await {
            Ok(()) => {}
            Err(e) => {
                tracing::error!("send error {} at {}:{}", e, file!(), line!())
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
        .join()
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
/// [zip_or_dir::ZipDirArchive::open_raw_or_gz].
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

/// This is our "real" main top-level function but we have some decoration we
/// need to do in [main], so we name this differently.
#[tracing::instrument]
pub async fn braid_offline_retrack(opt: Cli) -> anyhow::Result<()> {
    let data_src =
        braidz_parser::incremental_parser::IncrementalParser::open(opt.data_src.as_path())
            .with_context(|| {
                format!(
                    "while opening file \"{}\"",
                    opt.data_src.as_path().display()
                )
            })?;
    let data_src = data_src.parse_basics().with_context(|| {
        format!(
            "when parsing braidz file \"{}\"",
            opt.data_src.as_path().display()
        )
    })?;

    let tracking_params: flydra_types::TrackingParams = match opt.tracking_params {
        Some(ref fname) => {
            info!("reading tracking parameters from file {}", fname.display());
            // read the traking parameters
            let buf = std::fs::read_to_string(fname)
                .context(format!("loading tracking parameters {}", fname.display()))?;
            let tracking_params: flydra_types::TrackingParams = toml::from_str(&buf)?;
            tracking_params
        }
        None => {
            let parsed = data_src.basic_info();
            match parsed.tracking_params.clone() {
                Some(tp) => tp,
                None => {
                    let num_cams = data_src.basic_info().cam_info.camid2camn.len();
                    match num_cams {
                        0 => {
                            anyhow::bail!(
                                "No tracking parameters specified, none found in \
                            data_src, and no default is reasonable because zero cameras present."
                            )
                        }
                        1 => flydra_types::default_tracking_params_flat_3d(),
                        _ => flydra_types::default_tracking_params_full_3d(),
                    }
                }
            }
        }
    };
    let opts = KalmanizeOptions {
        start_frame: opt.start_frame,
        stop_frame: opt.stop_frame,
        ..Default::default()
    };

    // The user specifies an output .braidz file. But we will save initially to
    // a .braid directory. We here ensure the user's name had ".braidz"
    // extension and then calculate the name of the new output directory.
    let output_braidz = opt.output;

    // Raise an error if outputs exist.
    if output_braidz.exists() {
        return Err(anyhow::format_err!(
            "Path {} exists. Will not overwrite.",
            output_braidz.display()
        ));
    }

    let rt_handle = tokio::runtime::Handle::current();

    let save_performance_histograms = true;

    kalmanize(
        data_src,
        output_braidz,
        opt.fps.map(|v| NotNan::new(v).unwrap()),
        tracking_params,
        opts,
        rt_handle,
        save_performance_histograms,
        "braid-offline-retrack",
        opt.no_progress,
    )
    .await?;
    Ok(())
}

#[derive(Parser, Debug, Default)]
#[command(author, version, about)]
pub struct Cli {
    /// Input .braidz file
    #[arg(short = 'd', long)]
    pub data_src: std::path::PathBuf,
    /// Output file (must end with .braidz)
    #[arg(short = 'o', long)]
    pub output: std::path::PathBuf,
    /// Set frames per second
    #[arg(long)]
    pub fps: Option<f64>,
    /// Set start frame to start tracking
    #[arg(long)]
    pub start_frame: Option<u64>,
    /// Set stop frame to stop tracking
    #[arg(long)]
    pub stop_frame: Option<u64>,
    /// Tracking parameters TOML file.
    #[arg(long)]
    pub tracking_params: Option<std::path::PathBuf>,

    /// Disable display of progress indicator
    #[arg(long)]
    pub no_progress: bool,
}
