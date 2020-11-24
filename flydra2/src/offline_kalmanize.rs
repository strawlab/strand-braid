use log::{debug, info, warn};

use failure::ResultExt;
use std::{
    collections::BTreeMap,
    io::{Read, Seek, Write},
};

use crate::frame_bundler::StreamItem;
use crate::{
    pick_csvgz_or_csv2, run_func, CamInfoRow, CoordProcessor, Data2dDistortedRow, FrameData,
    FrameDataAndPoints, MyFloat, NumberedRawUdpPoint, Result, TrackingParams,
    CALIBRATION_XML_FNAME, CAM_INFO_CSV_FNAME, DATA2D_DISTORTED_CSV_FNAME,
};
use groupby::{AscendingGroupIter, BufferedSortIter};

use flydra_types::{RawCamName, RosCamName, SyncFno};

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

fn calc_fps_from_data<R: Read>(data_file: R) -> Result<Option<f64>> {
    let rdr = csv::Reader::from_reader(data_file);
    let mut data_iter = rdr.into_deserialize();
    let row0: Option<std::result::Result<Data2dDistortedRow, _>> = data_iter.next();
    if let Some(Ok(row0)) = row0 {
        let mut last_row = None;
        for row in data_iter {
            last_row = Some(row);
        }
        if let Some(res_last_row) = last_row {
            let last_row = res_last_row?;
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
                Ok(Some(df as f64 / dt))
            } else {
                // timestamp from host clock (should always be present)
                let dt =
                    last_row.cam_received_timestamp.as_f64() - row0.cam_received_timestamp.as_f64();
                Ok(Some(df as f64 / dt))
            }
        } else {
            debug!(
                "2d data: Single frame {}. {}:{}",
                row0.frame,
                file!(),
                line!()
            );
            Ok(None)
        }
    } else {
        debug!("no 2d data could be read. {}:{}", file!(), line!());
        Ok(None)
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

/// Perform tracking on the data
///
/// - `data_src` is the "flydra csv" format input directory. Create with
///   `convert_strand_cam_csv_to_flydra_csv_dir` or
///   `convert_flydra1_mainbrain_h5_to_csvdir`.
/// - `output_dirname` is the directory name (typically ending with `.braid`)
///   into which the resulting files will be saved. Upon closing all files, this
///   is typically zipped and saved to .braidz file.
pub async fn kalmanize<Q, R>(
    mut data_src: zip_or_dir::ZipDirArchive<R>,
    output_dirname: Q,
    expected_fps: Option<f64>,
    tracking_params: TrackingParams,
    opt2: KalmanizeOptions,
    rt_handle: tokio::runtime::Handle,
) -> Result<()>
where
    Q: AsRef<std::path::Path>,
    R: 'static + Read + Seek + Send,
{
    info!("tracking:");
    info!(
        "  {} -> {}",
        data_src.display(),
        output_dirname.as_ref().display()
    );

    let mut cal_fname = data_src.path_starter();
    cal_fname.push(CALIBRATION_XML_FNAME);
    cal_fname.set_extension("xml");

    let displayname = format!("{}", cal_fname.display());

    // read the calibration
    let cal_file = cal_fname
        .open()
        .context(format!("Could not open calibration file: {}", displayname))
        .map_err(|e| failure::Error::from(e))?;
    let recon = flydra_mvg::FlydraMultiCameraSystem::<MyFloat>::from_flydra_xml(cal_file)?;

    let mut cam_manager = crate::ConnectedCamerasManager::new(&Some(recon.clone()));

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
        old_image_fname.push(crate::IMAGES_DIRNAME);
        old_image_fname.push(&cam_name);
        old_image_fname.set_extension("png");

        if !old_image_fname.exists() {
            warn!("Image file {} not found", old_image_fname.display());
            continue;
        }

        let mut new_image_fname: std::path::PathBuf = output_dirname.as_ref().into();
        new_image_fname.push(crate::IMAGES_DIRNAME);
        std::fs::create_dir_all(&new_image_fname)?; // Create dir if needed.
        new_image_fname.push(&cam_name);
        new_image_fname.set_extension("png");

        let reader = old_image_fname.open()?;
        copy_to(reader, new_image_fname)?;
    }

    // open the data2d CSV file
    let mut data_fname = data_src.path_starter();
    data_fname.push(DATA2D_DISTORTED_CSV_FNAME);
    data_fname.set_extension("csv"); // below will also look for .csv.gz

    let fps = match expected_fps {
        Some(fps) => fps,
        None => {
            let data_file = pick_csvgz_or_csv2(&mut data_fname)?;

            // TODO: first choice parse "MainBrain running at {}" string (as in
            // braidz-parser). Second choice, do this.
            match calc_fps_from_data(data_file)? {
                Some(fps) => fps,
                None => {
                    return Err(failure::err_msg(
                        "frame rate could not be determined from data \
                        file and was not specified on command-line",
                    )
                    .into());
                }
            }
        }
    };

    let mut images = crate::ImageDictType::new();
    {
        // let mut image_filenames: Vec<zip_or_dir::PathLike<R>> = images_dirname.list_dir()?;
        let mut relnames = {
            let mut images_dirname = data_src.path_starter();
            images_dirname.push(crate::IMAGES_DIRNAME);

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
            fname.push(crate::IMAGES_DIRNAME);
            fname.push(&fname_str);

            let mut fd = fname.open()?;
            let mut buf = vec![];
            fd.read_to_end(&mut buf)?;

            images.insert(fname_str, buf);
        }
    }

    // read the cam_info CSV file
    let mut cam_info_fname = data_src.path_starter();
    cam_info_fname.push(CAM_INFO_CSV_FNAME);
    cam_info_fname.set_extension("csv"); // below will also look for .csv.gz
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
    let save_cfg = crate::StartSavingCsvConfig {
        out_dir: output_dirname.as_ref().into(),
        local: None,
        git_rev: env!("GIT_HASH").to_string(),
        fps: Some(fps as f32),
        images,
        print_stats: true,
    };

    write_controller.start_saving_data(save_cfg);

    let opt3 = opt2.clone();

    // send file to another thread because we want to read this into a stream
    // See https://github.com/alexcrichton/futures-rs/issues/49
    let reader_jh = std::thread::spawn(move || {
        run_func(move || -> Result<()> {
            // open the data2d CSV file
            let mut data_fname = data_src.path_starter();
            data_fname.push(DATA2D_DISTORTED_CSV_FNAME);
            data_fname.set_extension("csv"); // below will also look for .csv.gz

            log::trace!("loading data from {}", data_fname.display());

            let display_fname = format!("{}", data_fname.display());
            let data_file = pick_csvgz_or_csv2(&mut data_fname)?;
            let rdr = csv::Reader::from_reader(data_file);
            let data_iter = rdr.into_deserialize();

            let bufsize = 10000;
            let sorted_data_iter = BufferedSortIter::new(data_iter, bufsize)
                .context(format!("Reading rows from file: {}", display_fname))
                .map_err(|e| failure::Error::from(e))?;

            let data_row_frame_iter = AscendingGroupIter::new(sorted_data_iter);

            for data_frame_rows in data_row_frame_iter {
                // we are now in a loop where all rows come from the same frame, but not necessarily the same camera
                let data_frame_rows = data_frame_rows
                    .context(format!("Reading rows from file: {}", display_fname))
                    .map_err(|e| failure::Error::from(e))?;
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

            let model_server =
                crate::model_server::new_model_server(valve, None, &addr, info, rt_handle.clone())
                    .await?;
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
fn copy_to<R, P>(mut reader: R, dest: P) -> Result<()>
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
