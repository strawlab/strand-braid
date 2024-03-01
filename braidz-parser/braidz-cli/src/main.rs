use anyhow::Context;
use clap::{Parser, Subcommand};
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
            if let Some(cal) = &archive.calibration_info {
                if cal.water.is_some() {
                    tracing::error!("omitting water");
                }
                for (cam_name, cam) in cal.cameras.cams().iter() {
                    rec.log(
                        format!("world/camera/{cam_name}"),
                        &cam.rr_transform3d_archetype(),
                    )?;

                    match cam.rr_pinhole_archetype() {
                        Ok(pinhole) => {
                            rec.log(format!("world/camera/{cam_name}/raw_image"), &pinhole)?;
                        }
                        Err(e) => {
                            tracing::warn!("Could not convert camera calibration to rerun's pinhole model: {e}. \
                            Approximating the camera. When non-linear cameras are added to Rerun (see \
                            https://github.com/rerun-io/rerun/issues/2499), this code can be updated.");
                            let linearized_camera = cam.linearize_remove_skew()?;
                            let pinhole = linearized_camera.rr_pinhole_archetype()?;
                            rec.log(
                                format!("world/camera/{cam_name}/linearized_image"),
                                &pinhole,
                            )?;
                        }
                    };
                }
            }
            if let Some(kalman_estimates_table) = &archive.kalman_estimates_table {
                let mut trajectories =
                    std::collections::BTreeMap::<u32, Vec<(f32, f32, f32)>>::new();
                for row in kalman_estimates_table.iter() {
                    trajectories
                        .entry(row.obj_id)
                        .or_insert_with(Vec::new)
                        .push((row.x as f32, row.y as f32, row.z as f32));
                }

                for (obj_id, trajectory) in trajectories.iter() {
                    rec.log(
                        format!("world/obj_id/{obj_id}"),
                        &rerun::Points3D::new(trajectory.as_slice()),
                    )?;
                }
            }
            tracing::info!("Exported to Rerun RRD file: {}", output.display());
        }
    }
    Ok(())
}
