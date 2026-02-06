use camino::Utf8PathBuf;
use clap::{Parser, ValueEnum};
use eyre::WrapErr;
use rayon::prelude::*;

#[derive(Default, Debug, Parser)]
#[command(about, long_about = None)]
struct Opt {
    /// Output rrd filename. Defaults to "<INPUT>.rrd"
    #[arg(short, long)]
    output: Option<Utf8PathBuf>,

    /// Input filenames (.braidz and .mp4 files)
    inputs: Vec<Utf8PathBuf>,

    /// Should "linearized" (undistorted) MP4s be made from the original MP4s?
    ///
    /// If not, no MP4 is exported.
    #[arg(short, long)]
    export_linearized_mp4s: bool,

    /// If exporting MP4 files, which MP4 encoder should be be used?
    #[arg(long, value_enum, default_value_t)]
    encoder: Encoder,

    /// Print version
    #[arg(short, long)]
    version: bool,
}

#[derive(Debug, Default, Clone, PartialEq, ValueEnum)]
pub enum Encoder {
    #[default]
    LessAVC,
    OpenH264,
}

fn main() -> eyre::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        // SAFETY: We ensure that this only happens in single-threaded code
        // because this is immediately at the start of main() and no other
        // threads have started.
        unsafe { std::env::set_var("RUST_LOG", "info") };
    }
    env_tracing_logger::init();
    let opt = Opt::parse();
    export_rrd(opt)?;
    Ok(())
}

fn export_rrd(opt: Opt) -> eyre::Result<()> {
    if opt.version {
        println!(
            "{name} {version} (rerun {rerun_version})",
            name = env!("CARGO_PKG_NAME"),
            version = env!("CARGO_PKG_VERSION"),
            rerun_version = re_sdk::build_info().version,
        );
        return Ok(());
    }

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
            eyre::bail!("expected exactly one .braidz file, found {n_braidz_files}");
        } else {
            braidz_inputs[0].clone()
        }
    };
    inputs.remove(&input_braidz);

    let archive = braidz_parser::braidz_parse_path(&input_braidz)
        .with_context(|| format!("Parsing file {input_braidz}"))?;

    let output = output.unwrap_or_else(|| {
        let mut output = input_braidz.as_os_str().to_owned();
        output.push(".rrd");
        Utf8PathBuf::from_os_string(output).unwrap()
    });

    // Exclude expected output (e.g. from prior run) from inputs.
    inputs.remove(&output);
    // Exclude .linearized.mp4 files
    let inputs: Vec<_> = inputs
        .iter()
        .filter(|x| {
            !x.as_os_str()
                .to_string_lossy()
                .ends_with(braidz_rerun::UNDIST_NAME)
        })
        .collect();

    let mp4_inputs: Vec<_> = inputs
        .iter()
        .filter(|x| x.as_os_str().to_string_lossy().ends_with(".mp4"))
        .collect();
    if mp4_inputs.len() != inputs.len() {
        eyre::bail!("expected only mp4 inputs beyond one .braidz file.");
    }

    // Initiate recording
    let rec = re_sdk::RecordingStreamBuilder::new(env!("CARGO_PKG_NAME"))
        .save(&output)
        .with_context(|| format!("Creating output file {output}"))?;

    let have_image_data = !mp4_inputs.is_empty();
    let rrd_logger = braidz_rerun::braidz_into_rec(archive, rec, have_image_data)?;

    // Process videos
    mp4_inputs
        .as_slice()
        .par_iter()
        .try_for_each(|mp4_filename| {
            let my_mp4_writer = if opt.export_linearized_mp4s {
                let linearized_mp4_output = {
                    let output = mp4_filename.as_os_str().to_owned();
                    let output = output.to_str().unwrap().to_string();
                    let o2 = output.trim_end_matches(".mp4");
                    let output_ref: &std::ffi::OsStr = o2.as_ref();
                    let mut output = output_ref.to_os_string();
                    output.push(braidz_rerun::UNDIST_NAME);
                    Utf8PathBuf::from_os_string(output).unwrap()
                };

                tracing::info!("linearize (undistort) {mp4_filename} -> {linearized_mp4_output}");
                let out_fd = std::fs::File::create(&linearized_mp4_output)
                    .with_context(|| format!("Creating MP4 output file {linearized_mp4_output}"))?;

                let codec = if opt.encoder == Encoder::OpenH264 {
                    #[cfg(feature = "openh264-encode")]
                    {
                        use strand_cam_remote_control::OpenH264Preset;
                        strand_cam_remote_control::Mp4Codec::H264OpenH264(
                            strand_cam_remote_control::OpenH264Options {
                                debug: false,
                                preset: OpenH264Preset::AllFrames,
                            },
                        )
                    }
                    #[cfg(not(feature = "openh264-encode"))]
                    panic!("requested OpenH264 codec, but support for OpenH264 was not compiled.");
                } else {
                    strand_cam_remote_control::Mp4Codec::H264LessAvc
                };

                let cfg = strand_cam_remote_control::Mp4RecordingConfig {
                    codec,
                    max_framerate: Default::default(),
                    h264_metadata: None,
                };

                let my_mp4_writer = mp4_writer::Mp4Writer::new(out_fd, cfg, None)?;
                Some(my_mp4_writer)
            } else {
                None
            };

            let mp4_filename = mp4_filename.as_std_path().to_str().unwrap();
            rrd_logger.log_video(mp4_filename, my_mp4_writer)?;
            Ok::<(), eyre::ErrReport>(())
        })?;
    let re_version = re_sdk::build_info().version;
    tracing::info!("Exported to Rerun {re_version} RRD file: {output}");

    let rec = rrd_logger.close();
    rec.flush_blocking()?;

    Ok(())
}
