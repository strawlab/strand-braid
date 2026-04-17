use camino::Utf8PathBuf;
use clap::Parser;
use eyre::{Context, Result, eyre};
use mcsc_native::{ini_to_mcsc_config, load_mcsc_data, parse_mcsc_config, run_mcsc};

/// Multi-camera self-calibration tool
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to MCSC config directory containing multicamselfcal.cfg
    #[arg(short, long)]
    config: Utf8PathBuf,
}

fn main() -> Result<()> {
    {
        // Setup tracing but look just like a plain print for maximum
        // Octave-like readability.  Default to info level if RUST_LOG is not
        // set.
        if std::env::var_os("RUST_LOG").is_none() {
            // This is safe because we are at the start of main and no threads could
            // have been spawned yet.
            unsafe { std::env::set_var("RUST_LOG", "info") };
        }
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_target(false)
            .without_time()
            .with_level(false)
            .init();
    }
    let args = Args::parse();

    // Parse config
    let config = parse_mcsc_config(&args.config)?;

    tracing::debug!(
        "Parsed config: {} cameras, undo_radial={}, do_ba={}",
        config.num_cameras,
        config.undo_radial,
        config.do_ba
    );

    // Load data (includes Use-Nth-Frame subsampling)
    let input = load_mcsc_data(&config)?;

    // Convert to McscConfig
    let mcsc_config = ini_to_mcsc_config(&config);

    // Run MCSC
    let result = run_mcsc(input, mcsc_config)?;

    // Save results to result directory
    let result_dir = args
        .config
        .parent()
        .ok_or_else(|| eyre!("config path has no parent directory"))?
        .join("result");
    std::fs::create_dir_all(&result_dir)
        .with_context(|| format!("Failed to create result directory {}", result_dir))?;

    // Save projection matrices
    for (i, pmat) in result.projection_matrices.iter().enumerate() {
        let fname = result_dir.join(format!("camera{}.Pmat.cal", i + 1));
        let mut fd = std::fs::File::create(&fname)?;
        use std::io::Write;
        for r in 0..3 {
            let row: Vec<String> = (0..4).map(|c| format!("{:.15e}", pmat[(r, c)])).collect();
            fd.write_all(format!("{}\n", row.join(" ")).as_bytes())?;
        }
    }

    // Save points4cal files
    for (i, p4c) in result.points4cal.iter().enumerate() {
        let fname = result_dir.join(format!("cam{}.points4cal.dat", i + 1));
        let mut fd = std::fs::File::create(&fname)?;
        use std::io::Write;
        for row_idx in 0..p4c.nrows() {
            let vals: Vec<String> = (0..p4c.ncols())
                .map(|c| format!("{:.15e}", p4c[(row_idx, c)]))
                .collect();
            fd.write_all(format!("{}\n", vals.join(" ")).as_bytes())?;
        }
    }

    // Copy configuration files to result directory for flydra-mvg compatibility
    let config_dir = args.config.parent().unwrap();
    let src_res_path = config_dir.join("Res.dat");
    let dst_res_path = result_dir.join("Res.dat");
    std::fs::copy(&src_res_path, &dst_res_path)
        .with_context(|| format!("Failed to copy {} to {}", src_res_path, dst_res_path))?;

    let src_camera_order_path = config_dir.join("camera_order.txt");
    let dst_camera_order_path = result_dir.join("camera_order.txt");
    std::fs::copy(&src_camera_order_path, &dst_camera_order_path).with_context(|| {
        format!(
            "Failed to copy {} to {}",
            src_camera_order_path, dst_camera_order_path
        )
    })?;

    if config.undo_radial {
        for i in 0..config.num_cameras {
            let src_rad_path = config_dir.join(format!("basename{}.rad", i + 1));
            let dst_rad_path = result_dir.join(format!("basename{}.rad", i + 1));
            std::fs::copy(&src_rad_path, &dst_rad_path)
                .with_context(|| format!("Failed to copy {src_rad_path} to {dst_rad_path}"))?;
        }
    }

    tracing::info!("Results saved to {}", result_dir);

    // Test that we can load the saved results.
    let require_radfiles = config.undo_radial;
    flydra_mvg::read_mcsc_dir::<f64, _>(&result_dir, require_radfiles)
        .with_context(|| format!("while reading calibration at {result_dir}"))?;

    // Note that the saved results can have substantial skew.

    tracing::info!(
        "Calibration complete. {} cameras, {} inliers, {} 3D points",
        result.projection_matrices.len(),
        result.inlier_indices.len(),
        result.points_3d.ncols()
    );

    Ok(())
}
