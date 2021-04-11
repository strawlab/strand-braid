#![cfg_attr(feature = "backtrace", feature(backtrace))]

use log::{debug, info, warn};

use std::{
    collections::BTreeMap,
    io::{Read, Seek, Write},
};

use flydra_types::CamInfoRow;

use braidz_parser::pick_csvgz_or_csv2;

use flydra2::{
    run_func, CoordProcessor, Data2dDistortedRow, FrameData, FrameDataAndPoints,
    NumberedRawUdpPoint, StreamItem, SwitchingTrackingParams,
};
use groupby::{AscendingGroupIter, BufferedSortIter};

use flydra_types::{RawCamName, RosCamName, SyncFno};

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
            maybe_slope_eccentricty: maybe_slope_eccentricty,
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
        let ref mut rows_entry = by_cam.entry(inrow.camn).or_insert_with(|| Vec::new());
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
            if !last_row.timestamp.is_none() && !row0.timestamp.is_none() {
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
    tracking_params: SwitchingTrackingParams,
    opt2: KalmanizeOptions,
    rt_handle: tokio::runtime::Handle,
    save_performance_histograms: bool,
) -> Result<(), Error>
where
    Q: AsRef<std::path::Path>,
    R: 'static + Read + Seek + Send,
{
    let output_braidz = output_braidz.as_ref();
    let output_dirname = if output_braidz.extension() == Some(std::ffi::OsStr::new("braidz")) {
        let mut output_dirname: std::path::PathBuf = output_braidz.to_path_buf();
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

    let mut cam_manager = flydra2::ConnectedCamerasManager::new(&Some(recon.clone()));

    let (mut frame_data_tx, frame_data_rx) = futures::channel::mpsc::channel(0);
    let (save_data_tx, save_data_rx) = crossbeam_channel::unbounded();
    let save_empty_data2d = true;
    let ignore_latency = true;
    let mut coord_processor = CoordProcessor::new(
        cam_manager.clone(),
        Some(recon.clone()),
        tracking_params,
        save_data_tx,
        save_data_rx,
        save_empty_data2d,
        ignore_latency,
    )?;

    for cam_name in recon.cam_names() {
        let mut old_image_fname = data_src.path_starter();
        old_image_fname.push(flydra2::IMAGES_DIRNAME);
        old_image_fname.push(&cam_name);
        old_image_fname.set_extension("png");

        if !old_image_fname.exists() {
            warn!("Image file {} not found", old_image_fname.display());
            continue;
        }

        let mut new_image_fname: std::path::PathBuf = output_dirname.to_path_buf();
        new_image_fname.push(flydra2::IMAGES_DIRNAME);
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
            let data_file = pick_csvgz_or_csv2(&mut data_fname)?;

            // TODO: first choice parse "MainBrain running at {}" string (as in
            // braidz-parser). Second choice, do this.
            calc_fps_from_data(data_file)?
        }
    };

    let mut images = flydra2::ImageDictType::new();
    {
        // let mut image_filenames: Vec<zip_or_dir::PathLike<R>> = images_dirname.list_dir()?;
        let mut relnames = {
            let mut images_dirname = data_src.path_starter();
            images_dirname.push(flydra2::IMAGES_DIRNAME);

            // If no images are present, just skip them.
            match images_dirname.list_paths() {
                Ok(relnames) => relnames,
                Err(zip_or_dir::Error::NotDirectory) => vec![],
                Err(e) => return Err(e.into()),
            }
        };

        for relname in relnames.iter_mut() {
            let fname_str = format!("{}", relname.display());

            let mut fname = data_src.path_starter();
            fname.push(flydra2::IMAGES_DIRNAME);
            fname.push(&fname_str);

            let mut fd = fname.open()?;
            let mut buf = vec![];
            fd.read_to_end(&mut buf)?;

            images.insert(fname_str, buf);
        }
    }

    // read the cam_info CSV file
    let mut cam_info_fname = data_src.path_starter();
    cam_info_fname.push(flydra_types::CAM_INFO_CSV_FNAME);
    let cam_info_file = pick_csvgz_or_csv2(&mut cam_info_fname)?;
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

    let write_controller = coord_processor.get_write_controller();
    let save_cfg = flydra2::StartSavingCsvConfig {
        out_dir: output_dirname.to_path_buf(),
        local: None,
        git_rev: env!("GIT_HASH").to_string(),
        fps: Some(fps as f32),
        images,
        print_stats: true,
        save_performance_histograms,
    };

    write_controller.start_saving_data(save_cfg);

    let opt3 = opt2.clone();

    // send file to another thread because we want to read this into a stream
    // See https://github.com/alexcrichton/futures-rs/issues/49
    let reader_jh = std::thread::spawn(move || {
        run_func(move || -> Result<(), crate::Error> {
            // open the data2d CSV file
            let mut data_fname = data_src.path_starter();
            data_fname.push(flydra_types::DATA2D_DISTORTED_CSV_FNAME);

            log::trace!("loading data from {}", data_fname.display());

            let display_fname = format!("{}", data_fname.display());
            let data_file = pick_csvgz_or_csv2(&mut data_fname)?;
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
                    );
                    let fdp = FrameDataAndPoints { frame_data, points };
                    // block until sent
                    match futures::executor::block_on(futures::sink::SinkExt::send(
                        &mut frame_data_tx,
                        StreamItem::Packet(fdp),
                    )) {
                        Ok(()) => {}
                        Err(e) => return Err(e.into()),
                    }
                }
            }

            // block until sent
            match futures::executor::block_on(futures::sink::SinkExt::send(
                &mut frame_data_tx,
                StreamItem::EOF,
            )) {
                Ok(()) => {}
                Err(e) => return Err(e.into()),
            }

            Ok(())
        })
    });

    let expected_framerate = Some(fps as f32);

    // let model_server_addr = opt.model_server_addr.clone();

    let (_quit_trigger, valve) = stream_cancel::Valve::new();

    match &opt2.model_server_addr {
        Some(ref addr) => {
            let addr = addr.parse().unwrap();
            info!("send_pose server at {}", addr);
            let info = flydra_types::StaticMainbrainInfo {
                name: env!("CARGO_PKG_NAME").into(),
                version: env!("CARGO_PKG_VERSION").into(),
            };

            let model_server = flydra2::ModelServer::new(valve, None, &addr, info, rt_handle)?;
            coord_processor.add_listener(Box::new(model_server));
        }
        None => {}
    };

    // TODO: reorder incoming CSV lines to be monotonic w.r.t. frames? This
    // would cause behavior to diverge from online system but would result in
    // better retracking. Perhaps solution is to give runtime option to do
    // either.

    let consume_future = coord_processor.consume_stream(frame_data_rx, expected_framerate);

    let opt_jh = consume_future.await;

    // Allow writer thread time to finish writing.
    if let Some(jh) = opt_jh {
        jh.join().expect("join writer_thread_handle");
    }

    reader_jh.join().expect("join reader thread");

    Ok(())
}

/// Copy from `reader` to path `dest`.
fn copy_to<R, P>(mut reader: R, dest: P) -> flydra2::Result<()>
where
    R: Read,
    P: AsRef<std::path::Path>,
{
    let mut buf = vec![];
    let mut new_file = std::fs::File::create(dest)?;
    while reader.read(&mut buf)? > 0 {
        new_file.write(&mut buf)?;
    }
    new_file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
