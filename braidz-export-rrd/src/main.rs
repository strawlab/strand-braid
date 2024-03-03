use braidz_types::{camera_name_from_filename, CamInfo, CamNum};
use clap::Parser;
use color_eyre::eyre::{self as anyhow, WrapErr};
use frame_source::{ImageData, Timestamp};
use machine_vision_formats::{pixel_format, PixFmt};
use mvg::rerun_io::cam_geom_to_rr_pinhole_archetype as to_pinhole;
use ndarray::Array;
use std::path::PathBuf;

const SECONDS_TIMELINE: &str = "wall_clock";
const FRAMES_TIMELINE: &str = "frame";

#[derive(Debug, Parser)]
#[command(author, version)]
struct Opt {
    /// Output rrd filename. Defaults to "<INPUT>.rrd"
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Further input filenames
    inputs: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
struct QqqCamData {
    ent_path: String,
    use_intrinsics: Option<opencv_ros_camera::RosOpenCvIntrinsics<f64>>,
}

struct Qqq {
    rec: rerun::RecordingStream,
    cam_info: CamInfo,
    by_camn: std::collections::BTreeMap<CamNum, QqqCamData>,
    by_camname: std::collections::BTreeMap<String, QqqCamData>,
}

impl Qqq {
    fn new(rec: rerun::RecordingStream, cam_info: CamInfo) -> Self {
        Self {
            rec,
            cam_info,
            by_camn: Default::default(),
            by_camname: Default::default(),
        }
    }

    fn add_camera_calibration(
        &mut self,
        cam_name: &str,
        cam: &mvg::Camera<f64>,
    ) -> anyhow::Result<()> {
        let camn = self.cam_info.camid2camn.get(cam_name).unwrap();
        self.rec.log_timeless(
            format!("world/camera/{cam_name}"),
            &cam.rr_transform3d_archetype(),
        )?;

        let cam_data = match cam.rr_pinhole_archetype() {
            Ok(pinhole) => {
                let ent_path = format!("world/camera/{cam_name}/im");

                self.rec.log_timeless(ent_path.clone(), &pinhole)?;
                QqqCamData {
                    ent_path,
                    use_intrinsics: None,
                }
            }
            Err(e) => {
                tracing::warn!("Could not convert camera calibration to rerun's pinhole model: {e}. \
                            Approximating the camera. When non-linear cameras are added to Rerun (see \
                            https://github.com/rerun-io/rerun/issues/2499), this code can be updated.");
                let use_intrinsics = Some(cam.intrinsics().clone());
                let lin_cam = cam.linearize_to_cam_geom();
                let ent_path = format!("world/camera/{cam_name}/lin");
                self.rec.log_timeless(
                    ent_path.clone(),
                    &to_pinhole(&lin_cam, cam.width(), cam.height()),
                )?;
                QqqCamData {
                    ent_path,
                    use_intrinsics,
                }
            }
        };
        self.by_camn.insert(*camn, cam_data.clone());
        self.by_camname.insert(cam_name.to_string(), cam_data);
        Ok(())
    }

    fn log_video<P: AsRef<std::path::Path>>(&self, mp4_filename: P) -> anyhow::Result<()> {
        let (_, camname) = camera_name_from_filename(&mp4_filename);
        // could also get camname from title in movie metadata...
        dbg!(&camname);
        let camname = camname.unwrap();
        let cam_data = self.by_camname.get(&camname).unwrap();

        let do_decode_h264 = true;
        let mut src = frame_source::from_path(&mp4_filename, do_decode_h264)?;

        let start_time = src.frame0_time().unwrap();

        for frame in src.iter() {
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
            self.rec.disable_timeline(FRAMES_TIMELINE);
            self.rec
                .set_time_seconds(SECONDS_TIMELINE, stamp_flydra.as_f64());
            let image = to_rr_image(frame.into_image(), cam_data)?;
            self.rec.log(cam_data.ent_path.as_str(), &image)?;
        }

        Ok(())
    }

    fn log_data2d_distorted(&self, row: &braidz_types::Data2dDistortedRow) -> anyhow::Result<()> {
        if row.x.is_nan() {
            return Ok(());
        }
        let cam_data = self.by_camn.get(&row.camn).unwrap();

        self.rec.set_time_sequence(FRAMES_TIMELINE, row.frame);

        let dt = row.cam_received_timestamp.as_f64();
        self.rec.set_time_seconds(SECONDS_TIMELINE, dt);

        let arch = if let Some(nl_intrinsics) = &cam_data.use_intrinsics {
            let pt2d = cam_geom::Pixels::new(nalgebra::Vector2::new(row.x, row.y).transpose());
            let linearized = nl_intrinsics.undistort(&pt2d);
            let x = linearized.data[0];
            let y = linearized.data[1];
            // plot undistorted and distorted pixel while we are not undistorting image.
            rerun::Points2D::new([(x as f32, y as f32), (row.x as f32, row.y as f32)])
        } else {
            rerun::Points2D::new([(row.x as f32, row.y as f32)])
        };
        self.rec.log(cam_data.ent_path.as_str(), &arch)?;
        Ok(())
    }
}

fn to_rr_image(im: ImageData, camdata: &QqqCamData) -> anyhow::Result<rerun::Image> {
    let decoded = match im {
        ImageData::Decoded(decoded) => decoded,
        _ => anyhow::bail!("image not decoded"),
    };
    if camdata.use_intrinsics.is_some() {
        tracing::error!("undistort image not implemented.");
    }

    if true {
        // jpeg compression
        use basic_frame::DynamicFrame;
        let contents = basic_frame::match_all_dynamic_fmts!(
            &decoded,
            x,
            convert_image::frame_to_image(x, convert_image::ImageOptions::Jpeg(80),)
        )?;
        let format = Some(rerun::external::image::ImageFormat::Jpeg);
        Ok(rerun::Image::from_file_contents(contents, format).unwrap())
    } else {
        // Much larger file size but higher quality.
        let w = decoded.width() as usize;
        let h = decoded.height() as usize;

        let image = match decoded.pixel_format() {
            PixFmt::Mono8 => {
                let mono8 = decoded.into_pixel_format::<pixel_format::Mono8>()?;
                Array::from_vec(mono8.into()).into_shape((h, w, 1)).unwrap()
            }
            _ => {
                let rgb8 =
                    decoded.into_pixel_format::<machine_vision_formats::pixel_format::RGB8>()?;
                Array::from_vec(rgb8.into()).into_shape((h, w, 3)).unwrap()
            }
        };
        Ok(rerun::Image::try_from(image)?)
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
    let mut qqq = Qqq::new(rec.clone(), archive.cam_info.clone());

    // Process camera calibrations
    if let Some(cal) = &archive.calibration_info {
        if cal.water.is_some() {
            tracing::error!("omitting water");
        }
        for (cam_name, cam) in cal.cameras.cams().iter() {
            qqq.add_camera_calibration(cam_name, cam)?;
        }
    }

    // Process 2D point detections
    for row in archive.iter_data2d_distorted()? {
        let row = row?;
        qqq.log_data2d_distorted(&row)?;
    }

    // Process 3D kalman estimates
    let mut last_detection_per_obj = std::collections::BTreeMap::new();
    if let Some(kalman_estimates_table) = &archive.kalman_estimates_table {
        for row in kalman_estimates_table.iter() {
            rec.set_time_sequence(FRAMES_TIMELINE, i64::try_from(row.frame.0).unwrap());
            if let Some(timestamp) = &row.timestamp {
                rec.set_time_seconds(SECONDS_TIMELINE, timestamp.as_f64());
            }
            rec.log(
                format!("world/obj_id/{}", row.obj_id),
                &rerun::Points3D::new([(row.x as f32, row.y as f32, row.z as f32)]),
            )?;
            last_detection_per_obj.insert(row.obj_id, (row.frame, row.timestamp.clone()));
        }
    }
    // log end of trajectory - indicate there are no more data for this obj_id
    let empty_position: Vec<(f32, f32, f32)> = vec![];
    for (obj_id, (frame, timestamp)) in last_detection_per_obj.iter() {
        rec.set_time_sequence(FRAMES_TIMELINE, i64::try_from(frame.0).unwrap() + 1);
        if let Some(timestamp) = &timestamp {
            rec.set_time_seconds(
                SECONDS_TIMELINE,
                timestamp.as_f64() + inter_frame_interval_f64,
            );
        }
        rec.log(
            format!("world/obj_id/{}", obj_id),
            &rerun::Points3D::new(&empty_position),
        )?;
    }

    // Process videos
    for mp4_filename in mp4_inputs.iter() {
        qqq.log_video(mp4_filename)?;

        // rec.log("image", &rerun::Image::try_from(image)?)?;
    }
    tracing::info!("Exported to Rerun RRD file: {}", output.display());
    Ok(())
}
