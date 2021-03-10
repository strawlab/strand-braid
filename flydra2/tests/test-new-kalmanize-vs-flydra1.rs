use std::path::{Path, PathBuf};
use std::process::Command;

use failure::ResultExt;

use flydra2::KALMAN_ESTIMATES_FNAME;

fn unzip_into<P, Q>(src: P, dest: Q) -> Result<(), failure::Error>
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

fn move_path<P, Q>(src: P, dest: Q) -> Result<(), failure::Error>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    if !src.as_ref().exists() {
        return Err(failure::err_msg(format!("source does not exist")));
    }
    if src.as_ref().is_dir() {
        let mut options = fs_extra::dir::CopyOptions::new();
        options.overwrite = true;
        options.copy_inside = true;
        fs_extra::dir::move_dir(src, dest, &options)
            .map_err(|e| failure::err_msg(format!("move dir failed: {} {:?}", e, e)))?;
        Ok(())
    } else {
        match std::fs::rename(&src, &dest) {
            Ok(()) => Ok(()),
            Err(e) => {
                if e.raw_os_error() == Some(18) {
                    // "Invalid cross-device link"
                    std::fs::copy(&src, &dest).map_err(|e| {
                        failure::err_msg(format!("copy file failed: {} {:?}", e, e))
                    })?;
                    std::fs::remove_file(&src).map_err(|e| {
                        failure::err_msg(format!("remove file failed: {} {:?}", e, e))
                    })?;
                    Ok(())
                } else {
                    Err(failure::err_msg(format!(
                        "rename file failed: {} {:?}",
                        e, e
                    )))
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
) -> Result<(), failure::Error>
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
        let csv_path = src
            .as_ref()
            .join(KALMAN_ESTIMATES_FNAME)
            .with_extension("csv");
        let rdr = flydra2::pick_csvgz_or_csv(&csv_path)?;
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
                        return Err(failure::err_msg(e));
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

fn convert_csvdir_to_flydra1_mainbrain_h5<P, Q>(src: P, dest: Q) -> Result<(), failure::Error>
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

    let script = "../strand-braid-user/scripts/convert_kalmanized_csv_to_flydra_h5.py";
    let arg = format!("python {} {}", script, src.as_ref().display());
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
        return Err(failure::err_msg("python script failed"));
    }

    Ok(())
}

fn convert_flydra1_mainbrain_h5_to_csvdir<P, Q>(src: P, dest: Q) -> Result<(), failure::Error>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    // Compute the output location of the script by stripping the extension.
    let stem = src
        .as_ref()
        .file_stem()
        .ok_or_else(|| failure::err_msg("no file stem"))?;
    let parent = src
        .as_ref()
        .parent()
        .ok_or_else(|| failure::err_msg("no file parent"))?;

    let mut out_loc = PathBuf::from(parent);
    out_loc.push(stem);

    println!(
        "converting file {} -> {}",
        src.as_ref().display(),
        out_loc.to_string_lossy()
    );

    let script = "../strand-braid-user/scripts/export_h5_to_csv.py";
    let arg = format!("python {} {}", script, src.as_ref().display());
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
        move_path(&out_loc, dest)?;
    }

    if !output.status.success() {
        println!("{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        return Err(failure::err_msg("python script failed"));
    }

    Ok(())
}

#[cfg(test)]
async fn run_test(src: &str, untracked_dir: PathBuf) {
    convert_flydra1_mainbrain_h5_to_csvdir(src, &untracked_dir)
        .context(format!(
            "reading {} and saving to {}",
            src,
            untracked_dir.display()
        ))
        .unwrap();

    let tracked_dir = tempfile::tempdir().unwrap().into_path();

    let expected_fps = None;

    if tracked_dir.exists() {
        println!(
            "deleting pre-existing destination {}",
            tracked_dir.display()
        );
        std::fs::remove_dir_all(&tracked_dir).unwrap();
    }

    let tracking_params = flydra2::TrackingParams::default();
    println!("tracking with default parameters");

    let rt_handle = tokio::runtime::Handle::current();

    let data_src = zip_or_dir::ZipDirArchive::from_dir(untracked_dir).unwrap();

    flydra2::kalmanize(
        data_src,
        &tracked_dir,
        expected_fps,
        tracking_params,
        flydra2::KalmanizeOptions::default(),
        rt_handle,
    )
    .await
    .unwrap();
    println!("done tracking");

    unzip_into(tracked_dir.with_extension("braidz"), &tracked_dir).expect("unzip");

    let mut tracked_h5 = PathBuf::from(&tracked_dir);
    tracked_h5.set_extension("h5");

    // TODO: compare actual tracked 3D points and ensure mean error is not
    // larger than some amount? Or that mean reprojection error is not too
    // large?

    sanity_checks_csvdir(&tracked_dir, 71, 7649)
        .context(format!("sanity checks {}", tracked_dir.display()))
        .unwrap();

    convert_csvdir_to_flydra1_mainbrain_h5(&tracked_dir, &tracked_h5)
        .context(format!(
            "reading {} and saving to {}",
            tracked_dir.display(),
            tracked_h5.display()
        ))
        .unwrap();
}

#[tokio::test]
async fn do_test() {
    let _ = env_logger::builder().is_test(true).try_init();

    let src = "../_submodules/flydra/flydra_analysis/flydra_analysis/a2/sample_datafile-v0.4.28.h5";

    let untracked_dir = tempfile::tempdir().unwrap().into_path();

    run_test(src, untracked_dir).await;

    // TODO: check that results are similar to original.

    // TODO: check that filesize is roughly equal to original.
}

#[tokio::test]
async fn do_water_test() {
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

    let untracked_dir = tempfile::tempdir().unwrap().into_path();

    run_test(FNAME, untracked_dir).await;
    // TODO: check that results are similar to original.

    // TODO: check that filesize is roughly equal to original.
}
