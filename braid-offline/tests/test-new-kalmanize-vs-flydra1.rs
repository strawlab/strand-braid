use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Context;

use braid_offline::pick_csvgz_or_csv;

/// unzip the zip archive `src` into the destination `dest`.
///
/// The destination is created if it does not already exist.
fn unzip_into<P, Q>(src: P, dest: Q) -> Result<(), anyhow::Error>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let reader = std::fs::File::open(&src)?;
    let reader = std::io::BufReader::new(reader);
    let mut zip = zip::ZipArchive::new(reader)?;

    std::fs::create_dir_all(&dest)?;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;

        let out_name = dest.as_ref().join(file.name());

        if file.is_dir() {
            std::fs::create_dir_all(&out_name)?;
        } else {
            assert!(file.is_file());
            let out_fd = std::fs::File::create(out_name)?;
            let mut out_fd = std::io::BufWriter::new(out_fd);

            std::io::copy(&mut file, &mut out_fd)?;
        }
    }
    Ok(())
}

fn move_path<P, Q>(src: P, dest: Q) -> Result<(), anyhow::Error>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    if !src.as_ref().exists() {
        return Err(anyhow::anyhow!("source does not exist"));
    }
    if src.as_ref().is_dir() {
        let mut options = fs_extra::dir::CopyOptions::new();
        options.overwrite = true;
        options.copy_inside = true;
        fs_extra::dir::move_dir(src, dest, &options)
            .map_err(|e| anyhow::anyhow!("move dir failed: {} {:?}", e, e))?;
        Ok(())
    } else {
        match std::fs::rename(&src, &dest) {
            Ok(()) => Ok(()),
            Err(e) => {
                if e.raw_os_error() == Some(18) {
                    // "Invalid cross-device link"
                    std::fs::copy(&src, &dest)
                        .map_err(|e| anyhow::anyhow!("copy file failed: {} {:?}", e, e))?;
                    std::fs::remove_file(&src)
                        .map_err(|e| anyhow::anyhow!("remove file failed: {} {:?}", e, e))?;
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("rename file failed: {} {:?}", e, e))
                }
            }
        }
    }
}

#[cfg(test)]
fn sanity_checks_csvdir<P>(
    src: P,
    expected_num_obj_ids: usize,
    expected_num_rows: usize,
) -> Result<(), anyhow::Error>
where
    P: AsRef<Path>,
{
    println!(
        "sanity checks on {}, expected num obj_ids {}, expected num kest rows {}",
        src.as_ref().display(),
        expected_num_obj_ids,
        expected_num_rows
    );

    use flydra_types::{KalmanEstimatesRow, SyncFno};

    let kest_reader = {
        let csv_path = src.as_ref().join(flydra_types::KALMAN_ESTIMATES_CSV_FNAME);
        let rdr = pick_csvgz_or_csv(&csv_path)?;
        csv::Reader::from_reader(rdr)
    };

    use std::collections::HashMap;
    let mut current_frame: HashMap<u32, SyncFno> = HashMap::new();

    let mut actual_num_rows: usize = 0;
    for kest_row in kest_reader.into_deserialize() {
        actual_num_rows += 1;
        let kest_row: KalmanEstimatesRow = kest_row?;
        // println!("{:?}", kest_row);

        // For each obj_id, test that each row is one frame apart.
        //
        // This also ensures that:
        //  - each frame has each obj_id only once
        //  - each living obj_id is updated every frame
        //
        // It can be that obj_ids last only a few frames, though.
        {
            use std::collections::hash_map::Entry::*;
            match current_frame.entry(kest_row.obj_id) {
                Occupied(mut oe) => {
                    let cur_frame = oe.get_mut();
                    let diff = kest_row.frame.0 - cur_frame.0;
                    if diff != 1 {
                        let e = format!(
                            "For obj_id {}, frame {}, \
                            inter-frame interval is {}, not 1.",
                            kest_row.obj_id, kest_row.frame, diff
                        );
                        return Err(anyhow::anyhow!(e));
                    }
                    *cur_frame = kest_row.frame;
                }
                Vacant(ve) => {
                    ve.insert(kest_row.frame);
                }
            }
        }
    }

    approx::assert_relative_eq!(
        expected_num_rows as f64,
        actual_num_rows as f64,
        epsilon = std::f64::INFINITY,
        max_relative = 2.0,
    );

    let actual_num_obj_ids = current_frame.len();
    approx::assert_relative_eq!(
        expected_num_obj_ids as f64,
        actual_num_obj_ids as f64,
        epsilon = std::f64::INFINITY,
        max_relative = 2.0,
    );

    Ok(())
}

fn run_command(arg: &str) -> std::process::Output {
    if cfg!(target_os = "windows") {
        Command::new("cmd")
            .arg("/C")
            .arg(arg)
            .output()
            .expect("failed to execute process")
    } else {
        Command::new("sh")
            .arg("-c")
            .arg(arg)
            .output()
            .expect("failed to execute process")
    }
}

/// Convert `src` to `dest`. This will delete `src`.
fn convert_csvdir_to_flydra1_mainbrain_h5<P, Q>(src: P, dest: Q) -> Result<(), anyhow::Error>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let mut out_loc = PathBuf::from(src.as_ref());
    out_loc.set_extension("h5");

    if out_loc.exists() {
        println!("deleting pre-existing destination {}", out_loc.display());
        std::fs::remove_file(&out_loc)?;
    }

    println!(
        "converting dir {} -> {}",
        src.as_ref().display(),
        out_loc.display()
    );

    let script = "../strand-braid-user/scripts/convert_braidz_to_flydra_h5.py";
    let arg = format!("python {} {}", script, src.as_ref().display());

    // This will run the command, which will delete `src`.
    let output = run_command(&arg);

    if out_loc != dest.as_ref() {
        if dest.as_ref().exists() {
            println!(
                "deleting pre-existing destination {}",
                dest.as_ref().display()
            );
            std::fs::remove_file(dest.as_ref())?;
        }

        println!(
            "moving path {} -> {}",
            out_loc.to_string_lossy(),
            dest.as_ref().display()
        );

        assert!(out_loc.is_dir());
        move_path(&out_loc, dest)?;
    }

    if !output.status.success() {
        println!("{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        return Err(anyhow::anyhow!("python script failed"));
    }

    if src.as_ref().exists() {
        anyhow::bail!("src still exists after successful conversion.")
    }

    Ok(())
}

// The python script doesn't save this but we want it, so add it here.
fn add_metadata_to_csvdir<P>(dest_dir: P) -> Result<(), anyhow::Error>
where
    P: AsRef<Path>,
{
    let braid_metadata_path = dest_dir
        .as_ref()
        .to_path_buf()
        .join(flydra_types::BRAID_METADATA_YML_FNAME);

    let metadata = braidz_types::BraidMetadata {
        schema: flydra_types::BRAID_SCHEMA, // BraidMetadataSchemaTag
        git_revision: env!("GIT_HASH").to_string(),
        original_recording_time: None,
        save_empty_data2d: false, // We do filtering below, but is this correct?
        saving_program_name: env!("CARGO_PKG_NAME").to_string(),
    };
    let metadata_buf = serde_yaml::to_string(&metadata).unwrap();

    let mut fd = std::fs::File::create(&braid_metadata_path)?;
    fd.write_all(metadata_buf.as_bytes()).unwrap();

    Ok(())
}

fn convert_flydra1_mainbrain_h5_to_csvdir<P, Q>(src: P, dest: Q) -> Result<(), anyhow::Error>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    // Compute the output location of the script by stripping the extension.
    let stem = src
        .as_ref()
        .file_stem()
        .ok_or_else(|| anyhow::anyhow!("no file stem"))?;
    let parent = src
        .as_ref()
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no file parent"))?;

    let mut out_loc = PathBuf::from(parent);
    out_loc.push(stem);

    println!(
        "converting file {} -> {}",
        src.as_ref().display(),
        out_loc.to_string_lossy()
    );

    let script = "../strand-braid-user/scripts/export_h5_to_csv.py";
    let arg = format!("python {} {}", script, src.as_ref().display());
    // python script puts results in out_loc
    let output = run_command(&arg);

    if out_loc.exists() {
        if dest.as_ref().exists() {
            println!(
                "deleting pre-existing destination {}",
                dest.as_ref().display()
            );
            std::fs::remove_dir_all(dest.as_ref())?;
        }

        println!(
            "moving path {} -> {}",
            out_loc.to_string_lossy(),
            dest.as_ref().display()
        );

        assert!(out_loc.is_dir());
        move_path(&out_loc, &dest)?;
    }

    if !output.status.success() {
        println!("{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        return Err(anyhow::anyhow!("python script failed"));
    }

    // Add metadata file
    add_metadata_to_csvdir(&dest)?;

    Ok(())
}

#[cfg(test)]
async fn run_test(src: &str, untracked_dir: PathBuf) -> anyhow::Result<()> {
    convert_flydra1_mainbrain_h5_to_csvdir(src, &untracked_dir).context(format!(
        "reading {} and saving to {}",
        src,
        untracked_dir.display()
    ))?;

    let output_root = tempfile::tempdir()?; // will cleanup on drop
    let output_braidz = output_root.path().join("output.braidz");

    let expected_fps = None;

    let tracking_params = flydra_types::default_tracking_params_full_3d();
    println!("tracking with default 3D tracking parameters");

    let rt_handle = tokio::runtime::Handle::current();
    let data_src = braidz_parser::incremental_parser::IncrementalParser::open_dir(&untracked_dir)
        .unwrap_or_else(|_| panic!("While opening dir {}", untracked_dir.display()));
    let data_src = data_src
        .parse_basics()
        .with_context(|| format!("While reading dir: {}", untracked_dir.display()))?;

    let save_performance_histograms = true;

    braid_offline::kalmanize(
        data_src,
        &output_braidz,
        expected_fps,
        tracking_params,
        braid_offline::KalmanizeOptions::default(),
        rt_handle,
        save_performance_histograms,
        flydra2::BraidMetadataBuilder::saving_program_name(format!("{}:{}", file!(), line!())),
        true,
    )
    .await?;
    println!("done tracking");

    // expand .braidz file into /<root>/expanded.braid directory
    let tracked_dir = output_root.path().join("expanded.braid");
    unzip_into(output_braidz, &tracked_dir)?;

    // tracked_h5 becomes /<root>/expanded.h5
    let mut tracked_h5 = PathBuf::from(&tracked_dir);
    tracked_h5.set_extension("h5");

    // TODO: compare actual tracked 3D points and ensure mean error is not
    // larger than some amount? Or that mean reprojection error is not too
    // large?

    sanity_checks_csvdir(&tracked_dir, 71, 7649)
        .context(format!("sanity checks {}", tracked_dir.display()))?;

    convert_csvdir_to_flydra1_mainbrain_h5(&tracked_dir, &tracked_h5).context(format!(
        "reading {} and saving to {}",
        tracked_dir.display(),
        tracked_h5.display()
    ))?;

    // All temporary files are cleaned up as output_root is dropped.
    Ok(())
}

#[tokio::test]
async fn do_test() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(true).try_init();

    let src = "../_submodules/flydra/flydra_analysis/flydra_analysis/a2/sample_datafile-v0.4.28.h5";

    let untracked_dir = tempfile::tempdir()?.into_path(); // must manually cleanup

    run_test(src, untracked_dir.clone()).await?;

    // TODO: check that results are similar to original.

    // TODO: check that filesize is roughly equal to original.
    std::fs::remove_dir_all(untracked_dir)?;
    Ok(())
}

#[tokio::test]
async fn do_water_test() -> anyhow::Result<()> {
    const FNAME: &str = "20160527_163937.mainbrain-short.h5";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets";
    const SHA256SUM: &str = "7a63749cea63853ad1b9b2f6c32c087459a7be52aaef8730b0a41f00c5807d1b";
    let _ = env_logger::builder().is_test(true).try_init();

    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        FNAME,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )
    .unwrap();

    let untracked_dir = tempfile::tempdir().unwrap().into_path(); // must manually cleanup

    run_test(FNAME, untracked_dir.clone()).await?;
    // TODO: check that results are similar to original.

    // TODO: check that filesize is roughly equal to original.

    std::fs::remove_dir_all(untracked_dir).unwrap();
    Ok(())
}
