//! Convert 2D csv files from strand cam into tracks in .braid directory
#[macro_use]
extern crate log;

use std::{convert::TryInto, io::BufRead};

use serde::{Deserialize, Serialize};

use flydra2::{
    Data2dDistortedRow, MyFloat, TrackingParams, CALIBRATION_XML_FNAME, CAM_INFO_CSV_FNAME,
    DATA2D_DISTORTED_CSV_FNAME,
};
use flydra_types::CamInfoRow;
use strand_cam_csv_config_types::FullCfgFview2_0_26;
use strand_cam_pseudo_cal::PseudoCameraCalibrationData;

use anyhow::Result;

fn remove_trailing_newline(line1: &str) -> &str {
    if line1.ends_with("\n") {
        &line1[..line1.len() - 1]
    } else {
        &line1
    }
}

fn read_csv_commented_header<R>(point_detection_csv_reader: &mut R) -> Result<serde_yaml::Value>
where
    R: BufRead,
{
    enum ReadState {
        Initialized,
        FoundStartHeader,
        Reading(Vec<String>),
        Finished(std::result::Result<Vec<String>, anyhow::Error>),
        Marker,
    }
    impl ReadState {
        fn parse(&mut self, line1: &str) {
            let line = remove_trailing_newline(line1);
            let mut old = ReadState::Marker;
            std::mem::swap(self, &mut old);
            let next: ReadState = match old {
                ReadState::Initialized => {
                    if line.starts_with("#") {
                        if line == "# -- start of yaml config --" {
                            ReadState::FoundStartHeader
                        } else {
                            ReadState::Initialized
                        }
                    } else {
                        // *self = ReadState::Finished(Err(anyhow::format_err!("no header")));
                        ReadState::Finished(Ok(Vec::new()))
                    }
                }
                ReadState::FoundStartHeader => {
                    if line.starts_with("#") {
                        if line.starts_with("# ") {
                            let this_line = &line[2..];
                            ReadState::Reading(vec![this_line.to_string()])
                        } else {
                            ReadState::Finished(Err(
                                anyhow::format_err!("unexpected line prefix").into()
                            ))
                        }
                    } else {
                        ReadState::Finished(Err(anyhow::format_err!("premature end of headers")))
                    }
                }
                ReadState::Reading(mut vec_lines) => {
                    if line.starts_with("#") {
                        if line.starts_with("# ") {
                            if line == "# -- end of yaml config --" {
                                ReadState::Finished(Ok(vec_lines))
                            } else {
                                let this_line = &line[2..];
                                vec_lines.push(this_line.to_string());
                                ReadState::Reading(vec_lines)
                            }
                        } else {
                            ReadState::Finished(Err(anyhow::format_err!("unexpected line prefix")))
                        }
                    } else {
                        ReadState::Finished(Err(anyhow::format_err!("premature end of headers")))
                    }
                }
                ReadState::Finished(_) => {
                    ReadState::Finished(Err(anyhow::format_err!("parsing after finish")))
                }
                ReadState::Marker => {
                    ReadState::Finished(Err(anyhow::format_err!("parsing while parsing")))
                }
            };
            *self = next;
        }
        fn finish(self) -> std::result::Result<Vec<String>, anyhow::Error> {
            if let ReadState::Finished(rv) = self {
                rv
            } else {
                Err(anyhow::format_err!("premature end of header"))
            }
        }
    }

    let mut state = ReadState::Initialized;
    let mut this_line = String::new();
    loop {
        point_detection_csv_reader.read_line(&mut this_line)?;
        state.parse(&this_line);
        this_line.clear();
        if let ReadState::Finished(_) = &state {
            break;
        }
    }

    let header_lines = state.finish()?;
    let header = header_lines.join("\n");
    Ok(serde_yaml::from_str(&header)?)
}

pub enum StrandCamConfig {
    FullCfgFview2_0_25(strand_cam_csv_config_types::FullCfgFview2_0_25),
    FullCfgFview2_0_26(strand_cam_csv_config_types::FullCfgFview2_0_26),
}

impl StrandCamConfig {
    fn from_value(cfg: serde_yaml::Value) -> Result<StrandCamConfig> {
        match serde_yaml::from_value(cfg.clone()) {
            Ok(cfg26) => Ok(StrandCamConfig::FullCfgFview2_0_26(cfg26)),
            Err(err26) => {
                if let Ok(cfg25) = serde_yaml::from_value(cfg.clone()) {
                    Ok(StrandCamConfig::FullCfgFview2_0_25(cfg25))
                } else {
                    // Return parse error for latest version
                    Err(err26.into())
                }
            }
        }
    }

    fn to_latest(self) -> FullCfgFview2_0_26 {
        match self {
            StrandCamConfig::FullCfgFview2_0_25(cfg25) => config25_upgrade(cfg25),
            StrandCamConfig::FullCfgFview2_0_26(cfg26) => cfg26,
        }
    }
}

async fn kalmanize_2d<R>(
    point_detection_csv_reader: R,
    flydra_csv_temp_dir: Option<&tempdir::TempDir>,
    output_braidz: &std::path::Path,
    tracking_params: TrackingParams,
    to_recon_func: fn(
        serde_yaml::Value,
        &PseudoCalParams,
    ) -> Result<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
    to_ts0: fn(&serde_yaml::Value) -> Result<chrono::DateTime<chrono::Utc>>,
    pseudo_cal_params: &PseudoCalParams,
    rt_handle: tokio::runtime::Handle,
    row_filters: &Vec<RowFilter>,
) -> Result<()>
where
    R: BufRead,
{
    let mut owned_temp_dir = None;

    let flydra_csv_temp_dir = match flydra_csv_temp_dir {
        Some(x) => x,
        None => {
            owned_temp_dir = Some(tempdir::TempDir::new("tmp-strand-convert")?);
            owned_temp_dir.as_ref().unwrap()
        }
    };

    let num_points_converted = convert_strand_cam_csv_to_flydra_csv_dir(
        point_detection_csv_reader,
        to_recon_func,
        to_ts0,
        pseudo_cal_params,
        &flydra_csv_temp_dir,
        row_filters,
    )?;

    info!("    {} detected points converted.", num_points_converted);

    let data_src = zip_or_dir::ZipDirArchive::from_dir(flydra_csv_temp_dir.path().into())?;

    let save_performance_histograms = false;

    flydra2::kalmanize(
        data_src,
        output_braidz,
        None,
        tracking_params,
        flydra2::KalmanizeOptions::default(),
        rt_handle,
        save_performance_histograms,
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

fn convert_strand_cam_csv_to_flydra_csv_dir<R>(
    mut point_detection_csv_reader: R,
    to_recon_func: fn(
        serde_yaml::Value,
        &PseudoCalParams,
    ) -> Result<flydra_mvg::FlydraMultiCameraSystem<MyFloat>>,
    to_ts0: fn(&serde_yaml::Value) -> Result<chrono::DateTime<chrono::Utc>>,
    pseudo_cal_params: &PseudoCalParams,
    flydra_csv_temp_dir: &tempdir::TempDir,
    row_filters: &Vec<RowFilter>,
) -> Result<usize>
where
    R: BufRead,
{
    let header = read_csv_commented_header(&mut point_detection_csv_reader)?;
    let ts0 = to_ts0(&header)?;
    let recon = to_recon_func(header, pseudo_cal_params)?;

    assert_eq!(recon.len(), 1);

    // -------------------------------------------------
    let mut cal_path: std::path::PathBuf = flydra_csv_temp_dir.as_ref().to_path_buf();
    cal_path.push(CALIBRATION_XML_FNAME);
    cal_path.set_extension("xml");

    // let cam_name: String = recon.cams().keys().next().unwrap().clone();

    let fd = std::fs::File::create(&cal_path)?;
    // save calibration.xml file
    recon.to_flydra_xml(fd)?;

    // -------------------------------------------------
    // save cam_info.csv

    let mut csv_path = flydra_csv_temp_dir.as_ref().to_path_buf();
    csv_path.push(CAM_INFO_CSV_FNAME);
    csv_path.set_extension("csv");
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
    // save data2d_distorted.csv

    let mut rdr = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .from_reader(point_detection_csv_reader);

    let mut d2d_path = flydra_csv_temp_dir.as_ref().to_path_buf();
    d2d_path.push(DATA2D_DISTORTED_CSV_FNAME);
    d2d_path.set_extension("csv");
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
                    // reject points outside calibration region
                    if !is_inside_calibration_region(&record, &pseudo_cal_params) {
                        keep_row = false;
                        break;
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
    Ok(count)
}

#[inline]
fn is_inside_calibration_region(
    record: &Fview2CsvRecord,
    pseudo_cal_params: &PseudoCalParams,
) -> bool {
    let dist2 = (record.x_px - pseudo_cal_params.center_x as f64).powi(2)
        + (record.y_px - pseudo_cal_params.center_y as f64).powi(2);
    dist2 as f64 <= (pseudo_cal_params.radius as f64).powi(2)
}

fn config25_upgrade(
    orig: strand_cam_csv_config_types::FullCfgFview2_0_25,
) -> strand_cam_csv_config_types::FullCfgFview2_0_26 {
    strand_cam_csv_config_types::FullCfgFview2_0_26 {
        app: orig.app,
        camera: strand_cam_csv_config_types::CameraCfgFview2_0_26 {
            vendor: "default vendor".to_string(),
            model: "default model".to_string(),
            serial: "default serial".to_string(),
            width: 1280,
            height: 1024,
        },
        created_at: orig.created_at,
        csv_rate_limit: orig.csv_rate_limit,
        object_detection_cfg: orig.object_detection_cfg,
    }
}

fn to_ts0(cfg: &serde_yaml::Value) -> Result<chrono::DateTime<chrono::Utc>> {
    let cfg: FullCfgFview2_0_26 = StrandCamConfig::from_value(cfg.clone())?.to_latest();
    Ok(chrono::DateTime::with_timezone(
        &cfg.created_at,
        &chrono::Utc,
    ))
}

fn get_cam_name(cfg: &strand_cam_csv_config_types::CameraCfgFview2_0_26) -> &str {
    &cfg.serial
}

fn to_recon_func(
    cfg: serde_yaml::Value,
    pseudo_cal_params: &PseudoCalParams,
) -> Result<flydra_mvg::FlydraMultiCameraSystem<MyFloat>> {
    // let cfg: FullCfgFview2_0_26 = StrandCamConfig::from_value(cfg)?.to_latest();
    let cfg = StrandCamConfig::from_value(cfg)?.to_latest();

    let cam_name = get_cam_name(&cfg.camera);

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
    let system = cal_data.to_camera_system()?;
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

#[derive(Debug, Serialize, Deserialize)]
pub struct PseudoCalParams {
    pub physical_diameter_meters: f32,
    pub center_x: i16,
    pub center_y: i16,
    pub radius: u16,
}

/// Parse the configuration strings and run the kalman tracker
///
/// - `output_dirname` is used to initially create a "braid dir" (traditionally
///   ending with `.braid` in the name). Upon closing, this directory will be
///   converted to a file that ends with `.braidz`.
pub fn parse_configs_and_run<R>(
    point_detection_csv_reader: R,
    flydra_csv_temp_dir: Option<&tempdir::TempDir>,
    output_braidz: &std::path::Path,
    calibration_params_buf: &str,
    tracking_params_buf: Option<&str>,
    row_filters: &Vec<RowFilter>,
) -> Result<()>
where
    R: BufRead,
{
    let mut runtime = tokio::runtime::Builder::new()
        .threaded_scheduler()
        .enable_all()
        .build()
        .expect("runtime");

    let rt_handle = runtime.handle().clone();

    let tracking_params = match tracking_params_buf {
        Some(ref buf) => {
            let tracking_params: flydra_types::TrackingParams =
                toml::from_str(&buf).map_err(|e| anyhow::Error::from(e))?;
            tracking_params
        }
        None => flydra2::TrackingParams::default().into(),
    };

    let calibration_params =
        toml::from_str(&calibration_params_buf).map_err(|e| anyhow::Error::from(e))?;

    runtime.block_on(kalmanize_2d(
        point_detection_csv_reader,
        flydra_csv_temp_dir,
        output_braidz,
        tracking_params.try_into()?,
        to_recon_func,
        to_ts0,
        &calibration_params,
        rt_handle,
        &row_filters,
    ))
}
