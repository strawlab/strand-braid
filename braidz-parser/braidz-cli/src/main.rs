use braidz_types::{CamInfo, CamNum};
use clap::{Parser, Subcommand};
use color_eyre::eyre::{self as anyhow, WrapErr};
use mvg::rerun_io::cam_geom_to_rr_pinhole_archetype as to_pinhole;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[structopt(name = "braidz-cli")]
#[command(author, version)]
struct Opt {
    /// The command to run. Defaults to "print".
    #[command(subcommand)]
    command: Option<Commands>,

    /// Input braidz filename
    #[arg(global = true)]
    input: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Print a summary of the .braidz file
    Print {
        /// print all data in the `data2d_distorted` table
        #[arg(short, long)]
        data2d_distorted: bool,
    },
    /// Export an .rrd rerun file
    ExportRRD {
        /// Output rrd filename. Defaults to "<INPUT>.rrd"
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

struct QqqCamData {
    ent_path: String,
    use_intrinsics: Option<opencv_ros_camera::RosOpenCvIntrinsics<f64>>,
}

struct Qqq {
    rec: rerun::RecordingStream,
    cam_info: CamInfo,
    my_cam_data: std::collections::BTreeMap<CamNum, QqqCamData>,
}

impl Qqq {
    fn new(rec: rerun::RecordingStream, cam_info: CamInfo) -> Self {
        Self {
            rec,
            cam_info,
            my_cam_data: Default::default(),
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
        self.my_cam_data.insert(*camn, cam_data);
        Ok(())
    }

    fn log_data2d_distorted(&self, row: &braidz_types::Data2dDistortedRow) -> anyhow::Result<()> {
        if row.x.is_nan() {
            return Ok(());
        }
        let cam_data = self.my_cam_data.get(&row.camn).unwrap();

        self.rec.set_time_sequence("recording_sequence", row.frame);

        let dt = row.cam_received_timestamp.as_f64();
        self.rec.set_time_seconds("recording_time", dt);

        let arch = if let Some(nl_intrinsics) = &cam_data.use_intrinsics {
            let pt2d = cam_geom::Pixels::new(nalgebra::Vector2::new(row.x, row.y).transpose());
            let linearized = nl_intrinsics.undistort(&pt2d);
            let x = linearized.data[0];
            let y = linearized.data[1];
            rerun::Points2D::new([(x as f32, y as f32)])
        } else {
            rerun::Points2D::new([(row.x as f32, row.y as f32)])
        };
        self.rec.log(cam_data.ent_path.as_str(), &arch)?;
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info");
    }
    env_tracing_logger::init();
    let opt = Opt::parse();
    let command = opt.command.unwrap_or(Commands::Print {
        data2d_distorted: false,
    });
    let input = if let Some(input) = opt.input {
        input
    } else {
        anyhow::bail!("No <INPUT> given");
    };
    let attr = std::fs::metadata(&input)
        .with_context(|| format!("Getting file metadata for {}", input.display()))?;

    let mut archive = braidz_parser::braidz_parse_path(&input)
        .with_context(|| format!("Parsing file {}", input.display()))?;

    let summary =
        braidz_parser::summarize_braidz(&archive, input.display().to_string(), attr.len());

    match command {
        Commands::Print { data2d_distorted } => {
            let yaml_buf = serde_yaml::to_string(&summary)?;
            println!("{}", yaml_buf);

            if data2d_distorted {
                println!("data2d_distorted table: --------------");
                for row in archive.iter_data2d_distorted()? {
                    println!("{:?}", row);
                }
            }
        }
        Commands::ExportRRD { output } => {
            let output = output.unwrap_or_else(|| {
                let mut output = input.as_os_str().to_owned();
                output.push(".rrd");
                output.into()
            });

            let rec = rerun::RecordingStreamBuilder::new(env!("CARGO_PKG_NAME"))
                .save(&output)
                .with_context(|| format!("Creating output file {}", output.display()))?;
            let mut qqq = Qqq::new(rec.clone(), archive.cam_info.clone());
            if let Some(cal) = &archive.calibration_info {
                if cal.water.is_some() {
                    tracing::error!("omitting water");
                }
                for (cam_name, cam) in cal.cameras.cams().iter() {
                    qqq.add_camera_calibration(cam_name, cam)?;
                }
            }
            // let cam_info = &archive.cam_info;
            for row in archive.iter_data2d_distorted()? {
                let row = row?;
                qqq.log_data2d_distorted(&row)?;
            }

            if let Some(kalman_estimates_table) = &archive.kalman_estimates_table {
                for row in kalman_estimates_table.iter() {
                    rec.set_time_sequence(
                        "recording_sequence",
                        i64::try_from(row.frame.0).unwrap(),
                    );
                    if let Some(timestamp) = &row.timestamp {
                        rec.set_time_seconds("recording_time", timestamp.as_f64());
                    }
                    rec.log(
                        format!("world/obj_id/{}", row.obj_id),
                        &rerun::Points3D::new([(row.x as f32, row.y as f32, row.z as f32)]),
                    )?;
                }
            }
            tracing::info!("Exported to Rerun RRD file: {}", output.display());
        }
    }
    Ok(())
}
