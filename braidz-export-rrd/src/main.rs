use basic_frame::DynamicFrame;
use braidz_types::{camera_name_from_filename, CamNum};
use clap::Parser;
use color_eyre::eyre::{self as anyhow, WrapErr};
use frame_source::{ImageData, Timestamp};
use machine_vision_formats::{pixel_format, PixFmt};
use mp4_writer::Mp4Writer;
use mvg::rerun_io::cam_geom_to_rr_pinhole_archetype as to_pinhole;
use ndarray::Array;
use rayon::prelude::*;
use std::{collections::BTreeMap, path::PathBuf};

#[cfg(feature = "undistort-images")]
use crate::undistortion::UndistortionCache;

#[cfg(feature = "undistort-images")]
mod undistortion;

const SECONDS_TIMELINE: &str = "wall_clock";
const FRAMES_TIMELINE: &str = "frame";
const DETECT_NAME: &str = "detect";
const UNDIST_NAME: &str = ".linearized.mp4";

#[derive(Debug, Parser)]
#[command(author, version)]
struct Opt {
    /// Output rrd filename. Defaults to "<INPUT>.rrd"
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Further input filenames
    inputs: Vec<PathBuf>,

    #[arg(short, long)]
    export_linearized_mp4s: bool,

    #[arg(long)]
    openh264: bool,
}

#[cfg(feature = "undistort-images")]
const CAN_UNDISTORT_IMAGES: bool = true;
#[cfg(not(feature = "undistort-images"))]
const CAN_UNDISTORT_IMAGES: bool = false;

#[cfg(not(feature = "undistort-images"))]
struct UndistortionCache {}

#[derive(Clone, Debug)]
struct CachedCamData {
    image_ent_path: String,
    image_is_undistorted: bool,
    log_raw_2d_points: Option<String>,
    log_undistorted_2d_points: Option<String>,
    calibration: mvg::Camera<f64>,
    /// non-linear intrinsics, if present
    nl_intrinsics: Option<opencv_ros_camera::RosOpenCvIntrinsics<f64>>,
    camn: CamNum,
    #[allow(dead_code)]
    cam_name: String,
}

struct OfflineBraidzRerunLogger {
    rec: rerun::RecordingStream,
    camid2camn: BTreeMap<String, CamNum>,
    by_camn: BTreeMap<CamNum, CachedCamData>,
    by_camname: BTreeMap<String, CachedCamData>,
    frametimes: BTreeMap<CamNum, Vec<(i64, f64)>>,
    inter_frame_interval_f64: f64,
    have_image_data: bool,
    did_show_2499_warning: bool,
    /// Caches the frame number of the last data drawn for a given entity path.
    ///
    /// This is required because Rerun will continue showing an entity
    /// persistently after it was initially shown unless it is removed. This
    /// allows us to remove it.
    last_data2d: BTreeMap<String, i64>,
}

impl OfflineBraidzRerunLogger {
    fn new(
        rec: rerun::RecordingStream,
        camid2camn: BTreeMap<String, CamNum>,
        inter_frame_interval_f64: f64,
        have_image_data: bool,
    ) -> Self {
        Self {
            rec,
            camid2camn,
            by_camn: Default::default(),
            by_camname: Default::default(),
            frametimes: Default::default(),
            inter_frame_interval_f64,
            have_image_data,
            did_show_2499_warning: false,
            last_data2d: Default::default(),
        }
    }

    fn add_camera_calibration(
        &mut self,
        cam_name: &str,
        cam: &mvg::Camera<f64>,
    ) -> anyhow::Result<()> {
        let camn = self.camid2camn.get(cam_name).unwrap();
        self.rec.log_timeless(
            format!("world/camera/{cam_name}"),
            &cam.rr_transform3d_archetype(),
        )?;

        let base_path = format!("world/camera/{cam_name}");
        let raw_path = format!("{base_path}/raw");

        let cam_data = match cam.rr_pinhole_archetype() {
            Ok(pinhole) => {
                // Raw camera is linear. Life is easy.
                self.rec.log_timeless(raw_path.clone(), &pinhole)?;
                CachedCamData {
                    image_ent_path: raw_path.clone(),
                    image_is_undistorted: false,
                    log_raw_2d_points: Some(raw_path),
                    log_undistorted_2d_points: None,
                    calibration: cam.clone(),
                    nl_intrinsics: None,
                    camn: *camn,
                    cam_name: cam_name.to_string(),
                }
            }
            Err(mvg::MvgError::RerunUnsupportedIntrinsics) => {
                let lin_path = format!("{base_path}/lin"); // undistorted = linear
                if !self.did_show_2499_warning {
                    tracing::warn!(
                        "You have one or more cameras with distortion. While \
                        https://github.com/rerun-io/rerun/issues/2499 is not \
                        resolved, camera images, models, and coordinates will \
                        be linearized (undistorted)."
                    );
                    self.did_show_2499_warning = true;
                }
                let lin_cam = cam.linearize_to_cam_geom();
                self.rec.log_timeless(
                    lin_path.clone(),
                    &to_pinhole(&lin_cam, cam.width(), cam.height()),
                )?;

                let use_intrinsics = Some(cam.intrinsics().clone());

                let mut image_ent_path = lin_path.clone();
                let mut image_is_undistorted = true;

                let log_raw_2d_points = if self.have_image_data && !CAN_UNDISTORT_IMAGES {
                    // If we cannot undistort the images, also show the original
                    // image detection coordinates.
                    tracing::warn!(
                        "Cannot undistort images for {cam_name}. Logged images will contain \
                    distortion, but not logging distorted camera models. There will be some \
                    inconsistencies in the logged data."
                    );
                    image_ent_path = raw_path.clone();
                    image_is_undistorted = false;
                    Some(raw_path)
                } else {
                    None
                };

                // Always log the linear (a.k.a. undistorted) points.
                let log_undistorted_2d_points = Some(lin_path);

                CachedCamData {
                    image_ent_path,
                    image_is_undistorted,
                    calibration: cam.clone(),
                    log_raw_2d_points,
                    log_undistorted_2d_points,
                    nl_intrinsics: use_intrinsics,
                    camn: *camn,
                    cam_name: cam_name.to_string(),
                }
            }
            Err(e) => {
                return Err(e.into());
            }
        };
        self.by_camn.insert(*camn, cam_data.clone());
        self.by_camname.insert(cam_name.to_string(), cam_data);
        Ok(())
    }

    fn log_video<P: AsRef<std::path::Path>>(
        &self,
        mp4_filename: P,
        mut my_mp4_writer: Option<Mp4Writer<std::fs::File>>,
    ) -> anyhow::Result<()> {
        let (_, camname) = camera_name_from_filename(&mp4_filename);
        if camname.is_none() {
            tracing::warn!(
                "Did not recognize camera name for file \"{}\". Skipping.",
                mp4_filename.as_ref().display()
            );
            return Ok(());
        }
        // Should could get camname from title in movie metadata.
        let camname = camname.unwrap();
        let cam_data = self.by_camname.get(&camname).unwrap();

        let undist_cache = if let Some(intrinsics) = &cam_data.nl_intrinsics {
            #[cfg(not(feature = "undistort-images"))]
            {
                let _ = intrinsics; // silence unused warning.
                tracing::error!(
                    "Support to undistortion images was not compiled. \
                Images will be distorted but geometry will be linear."
                );
                None
            }
            #[cfg(feature = "undistort-images")]
            Some(UndistortionCache::new(
                intrinsics,
                cam_data.calibration.width(),
                cam_data.calibration.height(),
            )?)
        } else {
            None
        };

        let do_decode_h264 = true;
        let mut src = frame_source::from_path(&mp4_filename, do_decode_h264)?;
        tracing::info!(
            "Reading frames from {}: {}x{}",
            mp4_filename.as_ref().display(),
            src.width(),
            src.height()
        );
        assert_eq!(
            cam_data.calibration.width(),
            usize::try_from(src.width()).unwrap()
        );
        assert_eq!(
            cam_data.calibration.height(),
            usize::try_from(src.height()).unwrap()
        );
        let start_time = src.frame0_time().unwrap();
        let frametimes = self.frametimes.get(&cam_data.camn).unwrap();
        let (data2d_fnos, data2d_stamps): (Vec<i64>, Vec<f64>) = frametimes.iter().cloned().unzip();

        for (framecount, frame) in src.iter().enumerate() {
            let frame = frame?;
            let pts = match frame.timestamp() {
                Timestamp::Duration(pts) => pts,
                _ => {
                    anyhow::bail!("video has no PTS timestamps.");
                }
            };

            let stamp_chrono = start_time + pts;
            let stamp_flydra =
                flydra_types::FlydraFloatTimestampLocal::<flydra_types::Triggerbox>::from(
                    stamp_chrono,
                );
            let stamp_f64 = stamp_flydra.as_f64();
            let time_diffs: Vec<f64> = data2d_stamps
                .iter()
                .map(|x| (stamp_f64 - x).abs())
                .collect();
            let idx = argmin(&time_diffs).unwrap();
            let min_diff = time_diffs[idx];
            if min_diff <= (self.inter_frame_interval_f64 * 0.5) {
                let frameno = data2d_fnos[idx];
                self.rec.set_time_sequence(FRAMES_TIMELINE, frameno);
            } else {
                tracing::warn!(
                    "could not find Braid framenumber for video {}, video frame {}, timestamp {}",
                    mp4_filename.as_ref().display(),
                    framecount,
                    stamp_chrono
                );
                self.rec.disable_timeline(FRAMES_TIMELINE);
            }
            self.rec.set_time_seconds(SECONDS_TIMELINE, stamp_f64);
            let (image, decoded) = to_rr_image(frame.into_image(), undist_cache.as_ref())?;
            self.rec.log(cam_data.image_ent_path.clone(), &image)?;
            if let Some(my_mp4_writer) = my_mp4_writer.as_mut() {
                my_mp4_writer.write_dynamic(&decoded, stamp_chrono.into())?;
            }
        }
        Ok(())
    }

    fn log_data2d_distorted(
        &mut self,
        row: &braidz_types::Data2dDistortedRow,
    ) -> anyhow::Result<()> {
        // Always cache timing data.
        let cam_data = self.by_camn.get(&row.camn).unwrap();
        let dt = row.cam_received_timestamp.as_f64();
        self.frametimes
            .entry(cam_data.camn)
            .or_default()
            .push((row.frame, dt));

        self.rec.set_time_seconds(SECONDS_TIMELINE, dt);
        self.rec.set_time_sequence(FRAMES_TIMELINE, row.frame);
        let empty_position: [(f32, f32); 0] = [];

        if let Some(path_base) = &cam_data.log_raw_2d_points {
            let ent_path = format!("{path_base}/{DETECT_NAME}");
            if !row.x.is_nan() {
                self.rec.log(
                    ent_path.clone(),
                    &rerun::Points2D::new([(row.x as f32, row.y as f32)]),
                )?;
                self.last_data2d.insert(ent_path, row.frame);
            } else {
                // We have no detection at this frame. If required, tell rerun
                // to stop drawing previous detections now.
                if let Some(prior_frame) = self.last_data2d.remove(&ent_path) {
                    assert_eq!(
                        prior_frame + 1,
                        row.frame,
                        "must call in frame order for a given entity path"
                    );
                    self.rec
                        .log(ent_path, &rerun::Points2D::new(empty_position))?;
                }
            }
        };

        if let (Some(nl_intrinsics), Some(path_base)) = (
            &cam_data.nl_intrinsics,
            cam_data.log_undistorted_2d_points.as_ref(),
        ) {
            let ent_path = format!("{path_base}/{DETECT_NAME}");
            if !row.x.is_nan() {
                let pt2d = cam_geom::Pixels::new(nalgebra::Vector2::new(row.x, row.y).transpose());
                let linearized = nl_intrinsics.undistort(&pt2d);
                let x = linearized.data[0];
                let y = linearized.data[1];
                self.rec.log(
                    ent_path.clone(),
                    &rerun::Points2D::new([(x as f32, y as f32)]),
                )?;
                self.last_data2d.insert(ent_path, row.frame);
            } else {
                // We have no detection at this frame. If required, tell rerun
                // to stop drawing previous detections now.
                if let Some(prior_frame) = self.last_data2d.remove(&ent_path) {
                    assert_eq!(
                        prior_frame + 1,
                        row.frame,
                        "must call in frame order for a given entity path"
                    );
                    self.rec
                        .log(ent_path, &rerun::Points2D::new(empty_position))?;
                }
            }
        }
        Ok(())
    }

    fn log_kalman_estimates(
        &self,
        kalman_estimates_table: &[flydra_types::KalmanEstimatesRow],
        log_reprojected_2d: bool,
    ) -> anyhow::Result<()> {
        let mut last_detection_per_obj = BTreeMap::new();

        // iterate through all saved data.
        for row in kalman_estimates_table.iter() {
            self.rec
                .set_time_sequence(FRAMES_TIMELINE, i64::try_from(row.frame.0).unwrap());
            if let Some(timestamp) = &row.timestamp {
                self.rec
                    .set_time_seconds(SECONDS_TIMELINE, timestamp.as_f64());
            }
            self.rec.log(
                format!("world/obj_id/{}", row.obj_id),
                &rerun::Points3D::new([(row.x as f32, row.y as f32, row.z as f32)]),
            )?;
            last_detection_per_obj.insert(row.obj_id, (row.frame, row.timestamp.clone()));

            if log_reprojected_2d {
                for (_cam_name, cam_data) in self.by_camname.iter() {
                    // TODO: how to annotate this with row.obj_id?
                    let cam = &cam_data.calibration;
                    let pt3d = mvg::PointWorldFrame {
                        coords: nalgebra::Point3::new(row.x, row.y, row.z),
                    };
                    let arch = if cam_data.image_is_undistorted {
                        let pt2d = cam.project_3d_to_pixel(&pt3d).coords;
                        rerun::Points2D::new([(pt2d[0] as f32, pt2d[1] as f32)])
                    } else {
                        let pt2d = cam.project_3d_to_distorted_pixel(&pt3d).coords;
                        rerun::Points2D::new([(pt2d[0] as f32, pt2d[1] as f32)])
                    };
                    let ent_path = &cam_data.image_ent_path;
                    self.rec.log(format!("{ent_path}/reproj"), &arch)?;
                }
            }
        }

        // log end of trajectory - indicate there are no more data for this obj_id
        let empty_position: [(f32, f32, f32); 0] = [];
        for (obj_id, (frame, timestamp)) in last_detection_per_obj.iter() {
            self.rec
                .set_time_sequence(FRAMES_TIMELINE, i64::try_from(frame.0).unwrap() + 1);
            if let Some(timestamp) = &timestamp {
                self.rec.set_time_seconds(
                    SECONDS_TIMELINE,
                    timestamp.as_f64() + self.inter_frame_interval_f64,
                );
            }
            self.rec.log(
                format!("world/obj_id/{}", obj_id),
                &rerun::Points3D::new(empty_position),
            )?;
        }
        Ok(())
    }
}

fn to_rr_image(
    im: ImageData,
    undist_cache: Option<&UndistortionCache>,
) -> anyhow::Result<(rerun::Image, DynamicFrame)> {
    let decoded = match im {
        ImageData::Decoded(decoded) => decoded,
        _ => anyhow::bail!("image not decoded"),
    };

    let decoded: DynamicFrame = if let Some(undist_cache) = undist_cache {
        #[cfg(feature = "undistort-images")]
        {
            undistortion::undistort_image(decoded, undist_cache)?
        }
        #[cfg(not(feature = "undistort-images"))]
        {
            let _ = undist_cache; // silence unused variable warning.
            unreachable!();
        }
    } else {
        decoded
    };

    if true {
        // jpeg compression
        let contents = basic_frame::match_all_dynamic_fmts!(
            &decoded,
            x,
            convert_image::frame_to_image(x, convert_image::ImageOptions::Jpeg(80),)
        )?;
        let format = Some(rerun::external::image::ImageFormat::Jpeg);
        Ok((
            rerun::Image::from_file_contents(contents, format).unwrap(),
            decoded,
        ))
    } else {
        // Much larger file size but higher quality.
        let w = decoded.width() as usize;
        let h = decoded.height() as usize;

        let image = match decoded.pixel_format() {
            PixFmt::Mono8 => {
                let mono8 = decoded.clone().into_pixel_format::<pixel_format::Mono8>()?;
                Array::from_vec(mono8.into()).into_shape((h, w, 1)).unwrap()
            }
            _ => {
                let rgb8 = decoded
                    .clone()
                    .into_pixel_format::<machine_vision_formats::pixel_format::RGB8>()?;
                Array::from_vec(rgb8.into()).into_shape((h, w, 3)).unwrap()
            }
        };
        Ok((rerun::Image::try_from(image)?, decoded))
    }
}

fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }
    env_tracing_logger::init();
    let opt = Opt::parse();

    let output = opt.output;
    let inputs = opt.inputs;
    let mut inputs: std::collections::HashSet<_> = inputs.into_iter().collect();
    let input_braidz = {
        let braidz_inputs: Vec<_> = inputs
            .iter()
            .filter(|x| x.as_os_str().to_string_lossy().ends_with(".braidz"))
            .collect();
        let n_braidz_files = braidz_inputs.len();
        if n_braidz_files != 1 {
            anyhow::bail!("expected exactly one .braidz file, found {n_braidz_files}");
        } else {
            braidz_inputs[0].clone()
        }
    };
    inputs.remove(&input_braidz);

    let mut archive = braidz_parser::braidz_parse_path(&input_braidz)
        .with_context(|| format!("Parsing file {}", input_braidz.display()))?;

    let inter_frame_interval_f64 = 1.0 / archive.expected_fps;

    let output = output.unwrap_or_else(|| {
        let mut output = input_braidz.as_os_str().to_owned();
        output.push(".rrd");
        output.into()
    });

    // Exclude expected output (e.g. from prior run) from inputs.
    inputs.remove(&output);
    // Exclude .linearized.mp4 files
    let inputs: Vec<_> = inputs
        .iter()
        .filter(|x| !x.as_os_str().to_string_lossy().ends_with(UNDIST_NAME))
        .collect();

    let mp4_inputs: Vec<_> = inputs
        .iter()
        .filter(|x| x.as_os_str().to_string_lossy().ends_with(".mp4"))
        .collect();
    if mp4_inputs.len() != inputs.len() {
        anyhow::bail!("expected only mp4 inputs beyond one .braidz file.");
    }

    // Initiate recording
    let rec = rerun::RecordingStreamBuilder::new(env!("CARGO_PKG_NAME"))
        .save(&output)
        .with_context(|| format!("Creating output file {}", output.display()))?;

    // Create logger
    let mut rrd_logger = OfflineBraidzRerunLogger::new(
        rec,
        archive.cam_info.camid2camn.clone(),
        inter_frame_interval_f64,
        !mp4_inputs.is_empty(),
    );

    // Process camera calibrations
    if let Some(cal) = &archive.calibration_info {
        if cal.water.is_some() {
            tracing::error!("omitting water");
        }
        for (cam_name, cam) in cal.cameras.cams().iter() {
            rrd_logger.add_camera_calibration(cam_name, cam)?;
        }
    }

    // Process 2D point detections
    for row in archive.iter_data2d_distorted()? {
        let row = row?;
        rrd_logger.log_data2d_distorted(&row)?;
    }

    // Process 3D kalman estimates
    if let Some(kalman_estimates_table) = &archive.kalman_estimates_table {
        rrd_logger.log_kalman_estimates(kalman_estimates_table, true)?;
    }

    // Process videos
    mp4_inputs.as_slice().par_iter().for_each(|mp4_filename| {
        let my_mp4_writer = if opt.export_linearized_mp4s {
            let linearized_mp4_output: PathBuf = {
                let output = mp4_filename.as_os_str().to_owned();
                let output = output.to_str().unwrap().to_string();
                let o2 = output.trim_end_matches(".mp4");
                let output_ref: &std::ffi::OsStr = o2.as_ref();
                let mut output = output_ref.to_os_string();
                output.push(UNDIST_NAME);
                output.into()
            };

            tracing::info!(
                "linearize (undistort) {} -> {}",
                mp4_filename.display(),
                linearized_mp4_output.display()
            );
            let out_fd = std::fs::File::create(&linearized_mp4_output)
                .with_context(|| {
                    format!(
                        "Creating MP4 output file {}",
                        linearized_mp4_output.display()
                    )
                })
                .unwrap();

            let codec = if opt.openh264 {
                #[cfg(feature = "openh264-encode")]
                {
                    use ci2_remote_control::OpenH264Preset;
                    ci2_remote_control::Mp4Codec::H264OpenH264(
                        ci2_remote_control::OpenH264Options {
                            debug: false,
                            preset: OpenH264Preset::AllFrames,
                        },
                    )
                }
                #[cfg(not(feature = "openh264-encode"))]
                panic!("requested OpenH264 codec, but support for OpenH264 was not compiled.");
            } else {
                ci2_remote_control::Mp4Codec::H264LessAvc
            };

            let cfg = ci2_remote_control::Mp4RecordingConfig {
                codec,
                max_framerate: Default::default(),
                h264_metadata: None,
            };

            let my_mp4_writer = mp4_writer::Mp4Writer::new(out_fd, cfg, None).unwrap();
            Some(my_mp4_writer)
        } else {
            None
        };

        rrd_logger.log_video(mp4_filename, my_mp4_writer).unwrap();
    });
    tracing::info!("Exported to Rerun RRD file: {}", output.display());
    Ok(())
}

fn argmin(arr: &[f64]) -> Option<usize> {
    if arr.is_empty() {
        return None;
    }
    let mut idx = 0;
    let mut minval = arr[idx];
    for (i, val) in arr.iter().enumerate() {
        if val < &minval {
            minval = *val;
            idx = i;
        }
    }
    Some(idx)
}

#[test]
fn test_argmin() {
    assert_eq!(argmin(&[1.0, -1.0, 10.0]), Some(1));
    assert_eq!(argmin(&[]), None);
}
