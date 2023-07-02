//! Convert 2D csv files from strand cam into tracks in .braid directory
#[macro_use]
extern crate log;

use std::{
    collections::BTreeMap,
    io::{BufRead, Write},
    path::PathBuf,
};

use braidz_types::BraidMetadata;
use flydra_mvg::FlydraMultiCameraSystem;
use serde::{Deserialize, Serialize};

use flydra2::Data2dDistortedRow;
use flydra_types::{CamInfoRow, MyFloat, TrackingParams};
use strand_cam_csv_config_types::FullCfgFview2_0_26;
use strand_cam_pseudo_cal::PseudoCameraCalibrationData;

use anyhow::{Context, Result};

enum CalibrationType {
    SimpleCal(PseudoCalParams),
    FullCal(Box<FlydraMultiCameraSystem<f64>>),
}

#[allow(clippy::too_many_arguments)]
async fn kalmanize_2d<R>(
    mut point_detection_csv_reader: R,
    braid_csv_temp_dir: Option<&tempfile::TempDir>,
    flytrax_image: Option<image::DynamicImage>,
    output_braidz: &std::path::Path,
    tracking_params: TrackingParams,
    cal_file_name: &str,
    calibration_params_buf: &str,
    row_filters: &[RowFilter],
    no_progress: bool,
    eargs: Option<ExtrinsicsArgs>,
) -> Result<()>
where
    R: BufRead,
{
    let cfg = flytrax_io::read_csv_commented_header(&mut point_detection_csv_reader)?;

    let mut pseudo_cal_params = None;
    let cal_type = if cal_file_name.ends_with(".xml") {
        let full_cal = FlydraMultiCameraSystem::from_flydra_xml(calibration_params_buf.as_bytes())?;
        log::info!("loaded XML calibration with {} cameras", full_cal.len());
        CalibrationType::FullCal(Box::new(full_cal))
    } else if cal_file_name.ends_with(".toml") {
        let pseudo: PseudoCalParams =
            toml::from_str(calibration_params_buf).map_err(anyhow::Error::from)?;
        pseudo_cal_params = Some(pseudo.clone());

        CalibrationType::SimpleCal(pseudo)
    } else if cal_file_name.ends_with(".yaml") {
        let intrinsics: opencv_ros_camera::RosCameraInfo<f64> =
            serde_yaml::from_str(calibration_params_buf)?;
        log::info!("loaded YAML intrinsics calibration");

        let eargs = eargs.ok_or_else(|| {
            anyhow::anyhow!("when loading YAML calibration, need apriltags_3d_fiducial_coords")
        })?;

        let args = flytrax_apriltags_calibration::ComputeExtrinsicsArgs {
            apriltags_3d_fiducial_coords: eargs.apriltags_3d_fiducial_coords,
            flytrax_csv: eargs.flytrax_csv,
            image_filename: eargs.image_filename,
            intrinsics,
        };
        let single_cam_result = flytrax_apriltags_calibration::compute_extrinsics(&args)?;

        let system = single_cam_result.cal_result().cam_system.clone();

        for camera_name in system.cams_by_name().keys() {
            log::info!(
                "Calibration result for {}: {:.2} pixel mean reprojection distance",
                camera_name,
                single_cam_result.cal_result().mean_reproj_dist[camera_name]
            );
        }

        let full_cal = flydra_mvg::FlydraMultiCameraSystem::<f64>::from_system(system, None);

        CalibrationType::FullCal(Box::new(full_cal))
    } else {
        anyhow::bail!("unrecognized file extension for calibration: \"{cal_file_name}\"");
    };
    let recon = to_recon_func(&cfg, &cal_type)?;

    let images = {
        let mut images = BTreeMap::new();
        if let Some(flytrax_image) = flytrax_image {
            let cam_name = get_cam_name(&cfg.camera);
            images.insert(cam_name.to_string(), flytrax_image);
        }
        images
    };

    let mut owned_temp_dir = None;

    let braid_csv_temp_dir = match braid_csv_temp_dir {
        Some(x) => x,
        None => {
            owned_temp_dir = Some(
                tempfile::Builder::new()
                    .prefix("tmp-strand-convert")
                    .tempdir()?,
            );
            owned_temp_dir.as_ref().unwrap()
        }
    };

    let (num_points_converted, metadata) = convert_flytrax_csv_to_braid_csv_dir(
        cfg,
        recon,
        images,
        point_detection_csv_reader,
        pseudo_cal_params.as_ref(),
        braid_csv_temp_dir,
        row_filters,
    )?;

    info!("    {} detected points converted.", num_points_converted);

    let data_src =
        braidz_parser::incremental_parser::IncrementalParser::open_dir(braid_csv_temp_dir.path())?;
    let data_src = data_src.parse_basics().context(format!(
        "Failed parsing initial braidz information from {}",
        braid_csv_temp_dir.path().display()
    ))?;

    let save_performance_histograms = false;

    let metadata_builder = flydra2::BraidMetadataBuilder::Existing(metadata);

    braid_offline::kalmanize(
        data_src,
        output_braidz,
        None,
        tracking_params,
        braid_offline::KalmanizeOptions::default(),
        tokio::runtime::Handle::current(),
        save_performance_histograms,
        metadata_builder,
        no_progress,
    )
    .await?;

    if let Some(t) = owned_temp_dir {
        t.close()?;
    }

    Ok(())
}

/// These filters can be used to exclude data from being converted.
#[derive(Clone)]
pub enum RowFilter {
    /// Row is in time interval between start and stop
    InTimeInterval(
        flydra_types::FlydraFloatTimestampLocal<flydra_types::HostClock>,
        flydra_types::FlydraFloatTimestampLocal<flydra_types::HostClock>,
    ),
    /// Row is in region of calibration
    InPseudoCalRegion,
}

fn convert_flytrax_csv_to_braid_csv_dir<R>(
    cfg: FullCfgFview2_0_26,
    recon: FlydraMultiCameraSystem<f64>,
    images: BTreeMap<String, image::DynamicImage>,
    point_detection_csv_reader: R,
    pseudo_cal_params: Option<&PseudoCalParams>,
    braid_csv_temp_dir: &tempfile::TempDir,
    row_filters: &[RowFilter],
) -> Result<(usize, BraidMetadata)>
where
    R: BufRead,
{
    let ts0 = to_ts0(&cfg)?;

    assert_eq!(recon.len(), 1);

    // -------------------------------------------------
    let mut cal_path: std::path::PathBuf = braid_csv_temp_dir.as_ref().to_path_buf();
    cal_path.push(flydra_types::CALIBRATION_XML_FNAME);

    // let cam_name: String = recon.cams().keys().next().unwrap().clone();

    let fd = std::fs::File::create(&cal_path)?;
    // save calibration.xml file
    recon.to_flydra_xml(fd)?;

    // -------------------------------------------------
    // save cam_info.csv

    let mut csv_path = braid_csv_temp_dir.as_ref().to_path_buf();
    csv_path.push(flydra_types::CAM_INFO_CSV_FNAME);
    let fd = std::fs::File::create(&csv_path)?;
    let mut cam_info_wtr = csv::Writer::from_writer(fd);

    let cam_name = recon.cam_names().next().unwrap();

    let cam_info_rows: Vec<CamInfoRow> = vec![CamInfoRow {
        cam_id: cam_name.to_string(),
        camn: flydra_types::CamNum(0),
    }];
    for row in cam_info_rows.iter() {
        cam_info_wtr.serialize(row)?;
    }

    // -------------------------------------------------
    // save images/<cam>.png

    {
        let mut image_path = braid_csv_temp_dir.as_ref().to_path_buf();
        image_path.push(flydra_types::IMAGES_DIRNAME);
        std::fs::create_dir_all(&image_path)?;

        for (cam_name, data) in images.iter() {
            let fname = format!("{cam_name}.png");
            let fullpath = image_path.clone().join(fname);
            data.save(&fullpath)?;
        }
    }

    // -------------------------------------------------
    // save braid_metadata.yml

    let metadata = {
        let braid_metadata_path = braid_csv_temp_dir
            .as_ref()
            .to_path_buf()
            .join(flydra_types::BRAID_METADATA_YML_FNAME);

        let metadata = braidz_types::BraidMetadata {
            schema: flydra_types::BRAID_SCHEMA, // BraidMetadataSchemaTag
            git_revision: env!("GIT_HASH").to_string(),
            original_recording_time: Some(cfg.created_at),
            save_empty_data2d: false, // We do filtering below, but is this correct?
            saving_program_name: env!("CARGO_PKG_NAME").to_string(),
        };
        let metadata_buf = serde_yaml::to_string(&metadata).unwrap();

        let mut fd = std::fs::File::create(&braid_metadata_path)?;
        fd.write_all(metadata_buf.as_bytes()).unwrap();
        metadata
    };

    // -------------------------------------------------
    // save data2d_distorted.csv

    let mut rdr = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .from_reader(point_detection_csv_reader);

    let mut d2d_path = braid_csv_temp_dir.as_ref().to_path_buf();
    d2d_path.push(flydra_types::DATA2D_DISTORTED_CSV_FNAME);
    let fd = std::fs::File::create(&d2d_path)?;
    let mut writer = csv::Writer::from_writer(fd);
    let mut row_state = RowState::new();

    let mut count: usize = 0;
    for result in rdr.deserialize() {
        let record: Fview2CsvRecord = result?;

        let mut keep_row = true;
        for filter_row in row_filters.iter() {
            match filter_row {
                RowFilter::InTimeInterval(start, stop) => {
                    let this_time = get_timestamp(&record, &ts0);
                    if !(start.as_f64() <= this_time.as_f64()
                        && this_time.as_f64() <= stop.as_f64())
                    {
                        keep_row = false;
                        break;
                    }
                }
                RowFilter::InPseudoCalRegion => {
                    if let Some(pseudo_cal_params) = pseudo_cal_params.as_ref() {
                        // reject points outside calibration region
                        if !is_inside_calibration_region(&record, pseudo_cal_params) {
                            keep_row = false;
                            break;
                        }
                    }
                }
            }
        }

        if keep_row {
            let save = convert_row(record, &ts0, &mut row_state);
            writer.serialize(save)?;
            count += 1;
        }
    }
    Ok((count, metadata))
}

#[inline]
fn is_inside_calibration_region(
    record: &Fview2CsvRecord,
    pseudo_cal_params: &PseudoCalParams,
) -> bool {
    let dist2 = (record.x_px - pseudo_cal_params.center_x as f64).powi(2)
        + (record.y_px - pseudo_cal_params.center_y as f64).powi(2);
    dist2 <= (pseudo_cal_params.radius as f64).powi(2)
}

fn to_ts0(cfg: &FullCfgFview2_0_26) -> Result<chrono::DateTime<chrono::Utc>> {
    Ok(chrono::DateTime::with_timezone(
        &cfg.created_at,
        &chrono::Utc,
    ))
}

fn get_cam_name(cfg: &strand_cam_csv_config_types::CameraCfgFview2_0_26) -> &str {
    &cfg.model
}

fn to_recon_func(
    cfg: &FullCfgFview2_0_26,
    cal_type: &CalibrationType,
) -> Result<flydra_mvg::FlydraMultiCameraSystem<MyFloat>> {
    let cam_name = get_cam_name(&cfg.camera);

    let system = match cal_type {
        CalibrationType::SimpleCal(pseudo_cal_params) => {
            let cal_data = PseudoCameraCalibrationData {
                cam_name: flydra_types::RawCamName::new(cam_name.to_string()),
                width: cfg.camera.width,
                height: cfg.camera.height,
                physical_diameter_meters: pseudo_cal_params.physical_diameter_meters,
                image_circle: http_video_streaming_types::CircleParams {
                    center_x: pseudo_cal_params.center_x,
                    center_y: pseudo_cal_params.center_y,
                    radius: pseudo_cal_params.radius,
                },
            };

            cal_data.to_camera_system()?
        }
        CalibrationType::FullCal(system) => *system.clone(),
    };
    Ok(system)
}

struct RowState {
    this_frame: i64,
    next_idx: u8,
}

impl RowState {
    fn new() -> Self {
        Self {
            this_frame: 0,
            next_idx: 0,
        }
    }
    fn update(&mut self, frame: i64) -> u8 {
        let mut next = 0;
        if frame == self.this_frame {
            next = self.next_idx;
            self.next_idx += 1;
        } else {
            assert!(frame > self.this_frame);
            self.this_frame = frame;
            self.next_idx = 1;
        }
        next
    }
}

fn get_timestamp(
    strand_cam_row: &Fview2CsvRecord,
    ts0: &chrono::DateTime<chrono::Utc>,
) -> flydra_types::FlydraFloatTimestampLocal<flydra_types::HostClock> {
    let toffset = chrono::Duration::microseconds(strand_cam_row.time_microseconds);
    let dt = *ts0 + toffset;
    flydra_types::FlydraFloatTimestampLocal::from_dt(&dt)
}

// maybe use Data2dDistortedRowF32 ?
fn convert_row(
    strand_cam_row: Fview2CsvRecord,
    ts0: &chrono::DateTime<chrono::Utc>,
    row_state: &mut RowState,
) -> Data2dDistortedRow {
    let (eccentricity, slope) = match strand_cam_row.orientation_radians_mod_pi {
        Some(angle) => (1.1, angle.tan()),
        None => (std::f64::NAN, std::f64::NAN),
    };
    let frame_pt_idx = row_state.update(strand_cam_row.frame);
    Data2dDistortedRow {
        area: strand_cam_row.central_moment.unwrap_or(std::f64::NAN),
        cam_received_timestamp: get_timestamp(&strand_cam_row, ts0),
        device_timestamp: None,
        block_id: None,
        camn: flydra_types::CamNum(0),
        cur_val: 255,
        frame: strand_cam_row.frame,
        eccentricity,
        frame_pt_idx,
        mean_val: std::f64::NAN,
        slope,
        sumsqf_val: std::f64::NAN,
        timestamp: None, //flydra_types::FlydraFloatTimestampLocal::from_dt(&dt),
        x: strand_cam_row.x_px,
        y: strand_cam_row.y_px,
    }
}

#[derive(Debug, Deserialize)]
struct Fview2CsvRecord {
    time_microseconds: i64,
    frame: i64,
    central_moment: Option<f64>,
    orientation_radians_mod_pi: Option<f64>,
    x_px: f64,
    y_px: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PseudoCalParams {
    pub physical_diameter_meters: f32,
    pub center_x: i16,
    pub center_y: i16,
    pub radius: u16,
}

pub struct ExtrinsicsArgs {
    pub apriltags_3d_fiducial_coords: PathBuf,
    pub flytrax_csv: PathBuf,
    pub image_filename: PathBuf,
}

/// Parse the configuration strings and run the kalman tracker
///
/// - `output_braidz` is used to initially create a "braid dir" (typically
///   ending with `.braid` in the name). Upon closing, this directory will be
///   converted to a file that ends with `.braidz`.
#[allow(clippy::too_many_arguments)]
pub async fn parse_configs_and_run<R>(
    point_detection_csv_reader: R,
    flydra_csv_temp_dir: Option<&tempfile::TempDir>,
    flytrax_image: Option<image::DynamicImage>,
    output_braidz: &std::path::Path,
    cal_file_name: &str,
    calibration_params_buf: &str,
    tracking_params_buf: Option<&str>,
    row_filters: &[RowFilter],
    no_progress: bool,
    eargs: Option<ExtrinsicsArgs>,
) -> Result<()>
where
    R: BufRead,
{
    let tracking_params = match tracking_params_buf {
        Some(buf) => {
            let tracking_params: flydra_types::TrackingParams =
                toml::from_str(buf).map_err(anyhow::Error::from)?;
            tracking_params
        }
        None => flydra_types::default_tracking_params_flat_3d(),
    };

    kalmanize_2d(
        point_detection_csv_reader,
        flydra_csv_temp_dir,
        flytrax_image,
        output_braidz,
        tracking_params,
        cal_file_name,
        calibration_params_buf,
        row_filters,
        no_progress,
        eargs,
    )
    .await
}
