use clap::Parser;
use eyre::{self, Context, Result};
use polars::prelude::*;
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

use flydra_mvg::FlydraMultiCameraSystem;
use mcsc_structs::{DatMat, McscCfg, McscConfigDir, RadFile};

#[derive(Parser, Default)]
struct Cli {
    /// Input braidz filename.
    #[arg(long)]
    input: PathBuf,

    /// Input directory to be searched for YAML calibration files from
    /// checkerboard calibration. (Typically
    /// "~/.config/strand-cam/camera_info").
    #[arg(long)]
    checkerboard_cal_dir: Option<PathBuf>,

    #[arg(long)]
    force_allow_no_checkerboard_cal: bool,

    /// Rather than using each frame, use only 1/N of them.
    #[arg(long)]
    use_nth_observation: Option<u16>,

    /// If set, keep the intermediate MCSC calibration directory.
    #[arg(long)]
    keep: bool,
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> io::Result<()> {
    // modified from https://stackoverflow.com/a/65192210
    let entries: Vec<fs::DirEntry> = fs::read_dir(src)?
        .into_iter()
        .collect::<io::Result<Vec<fs::DirEntry>>>()?;
    fs::create_dir_all(&dst)?;
    for entry in entries.iter() {
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    env_tracing_logger::init();
    let opt = Cli::parse();
    let xml_out_name = braiz_mcsc(opt)?;
    println!(
        "Unaligned calibration XML saved to {}",
        xml_out_name.display()
    );
    Ok(())
}

fn braiz_mcsc(opt: Cli) -> Result<PathBuf> {
    let use_nth_observation = opt.use_nth_observation.unwrap_or(1);

    let mut archive = zip_or_dir::ZipDirArchive::auto_from_path(&opt.input)
        .with_context(|| format!("Parsing file {}", opt.input.display()))?;

    let camid2camn_df = {
        // Read data2d_distorted to memory.
        let cursor = {
            let data_fname = archive
                .path_starter()
                .join(flydra_types::CAM_INFO_CSV_FNAME);
            let mut rdr = braidz_parser::open_maybe_gzipped(data_fname)?;
            let mut buf = Vec::new();
            rdr.read_to_end(&mut buf)?;
            std::io::Cursor::new(buf)
        };

        polars_io::csv::read::CsvReadOptions::default()
            .with_has_header(true)
            .into_reader_with_file_handle(cursor)
            .finish()?
    };

    let mut camn2cam_id = BTreeMap::new();
    let mut images = BTreeMap::new();
    let mut camera_order = vec![];
    let mut res = vec![];
    for cam_id in camid2camn_df["cam_id"].str()?.iter() {
        let cam_id = cam_id.unwrap();
        camera_order.push(cam_id.to_string());

        let image_fname = archive
            .path_starter()
            .join(flydra_types::IMAGES_DIRNAME)
            .join(format!("{cam_id}.png"));

        let mut rdr = braidz_parser::open_maybe_gzipped(image_fname)?;
        let mut im_buf = Vec::new();
        rdr.read_to_end(&mut im_buf)?;

        let im = image::load_from_memory(&im_buf)?;
        res.push(im.width() as usize);
        res.push(im.height() as usize);
        images.insert(cam_id, im);
    }
    let camns: Vec<i64> = camid2camn_df["camn"]
        .i64()?
        .iter()
        .map(|x| x.unwrap())
        .collect();
    for (camn, cam_id) in camns.iter().zip(camera_order.iter()) {
        camn2cam_id.insert(*camn, cam_id.clone());
    }

    let radfiles = if let Some(checkerboard_cal_dir) = &opt.checkerboard_cal_dir {
        if opt.force_allow_no_checkerboard_cal {
            eyre::bail!("--checkerboard-cal-dir was specified but --force-allow-no-checkerboard-cal is set.");
        }
        let mut radfiles = vec![];

        for cam_id in camid2camn_df["cam_id"].str()?.iter() {
            let cam_id = cam_id.unwrap();
            let yaml_intrinsics_fname = checkerboard_cal_dir.join(&format!("{cam_id}.yaml"));
            let yaml_buf = std::fs::read_to_string(&yaml_intrinsics_fname)
                .with_context(|| format!("while reading {}", yaml_intrinsics_fname.display()))?;

            let intrinsics: opencv_ros_camera::RosCameraInfo<f64> = serde_yaml::from_str(&yaml_buf)
                .with_context(|| format!("while parsing {}", yaml_intrinsics_fname.display()))?;

            radfiles.push(RadFile::new(&intrinsics)?);

            let im = &images[cam_id];
            let w: usize = im.width().try_into().unwrap();
            let h: usize = im.height().try_into().unwrap();
            if intrinsics.image_width != w {
                eyre::bail!("PNG image resolution does not match YAML file.")
            }
            if intrinsics.image_height != h {
                eyre::bail!("PNG image resolution does not match YAML file.")
            }
        }

        radfiles
    } else {
        if opt.force_allow_no_checkerboard_cal {
            vec![]
        } else {
            eyre::bail!(
                "No --checkerboard-cal-dir given and --force-allow-no-checkerboard-cal not set."
            );
        }
    };

    let num_cameras = camid2camn_df.height();
    assert_eq!(num_cameras, camns.len());

    let data2d_df = {
        // Read data2d_distorted to memory.
        let cursor = {
            let data_fname = archive
                .path_starter()
                .join(flydra_types::DATA2D_DISTORTED_CSV_FNAME);
            let mut rdr = braidz_parser::open_maybe_gzipped(data_fname)?;
            let mut buf = Vec::new();
            rdr.read_to_end(&mut buf)?;
            std::io::Cursor::new(buf)
        };

        let mut data2d_df = polars_io::csv::read::CsvReadOptions::default()
            .with_has_header(true)
            .into_reader_with_file_handle(cursor)
            .finish()?;

        let drop_columns = [
            "cam_received_timestamp",
            "device_timestamp",
            "block_id",
            "area",
            "slope",
            "frame_pt_idx",
            "eccentricity",
            "cur_val",
            "mean_val",
            "sumsqf_val",
        ];

        for colname in drop_columns {
            if data2d_df.get_column_index(colname).is_some() {
                data2d_df.drop_in_place(colname)?;
            }
        }
        let cond = data2d_df["x"].is_not_nan()?;
        data2d_df.filter(&cond)?
    };

    // In this scope, we collect points for calibration.
    let (id_mat, points) = {
        let mut points = vec![];
        let mut id_mat = vec![];
        let mut count = 0;
        let mut by_camn = BTreeMap::new();
        let mut by_n_pts = BTreeMap::new();

        // Iterate over frames
        for gdf in data2d_df.partition_by_stable(["frame"], true)?.iter() {
            // need at least 3 cameras for data to be useful to MCSC
            if gdf["camn"].unique()?.len() < 3 {
                continue;
            }

            let this_camns: Vec<i64> = gdf
                .column("camn")
                .unwrap()
                .i64()
                .unwrap()
                .into_iter()
                .map(|x| x.unwrap())
                .collect();
            let gx: Vec<f64> = gdf
                .column("x")
                .unwrap()
                .f64()
                .unwrap()
                .into_iter()
                .map(|x| x.unwrap())
                .collect();
            let gy: Vec<f64> = gdf
                .column("y")
                .unwrap()
                .f64()
                .unwrap()
                .into_iter()
                .map(|x| x.unwrap())
                .collect();
            let mut this_frame_n_cams = 0;
            for camn in camns.iter() {
                let idx = this_camns.iter().position(|x| x == camn);
                if let Some(idx) = idx {
                    id_mat.push(1);
                    points.push(gx[idx]);
                    points.push(gy[idx]);
                    points.push(1.0);

                    let cam_entry = by_camn.entry(camn).or_insert(0usize);
                    *cam_entry += 1;
                    this_frame_n_cams += 1;
                } else {
                    // no data for this camn on this frame
                    id_mat.push(0);
                    points.push(-1.0);
                    points.push(-1.0);
                    points.push(-1.0);
                }
            }
            count += 1;
            let npt_entry = by_n_pts.entry(this_frame_n_cams).or_insert(0usize);
            *npt_entry += 1;
        }

        println!("{count} points");
        println!("by camera id:");
        for (camn, count_per_cam) in by_camn.iter() {
            let cam_id = &camn2cam_id[*camn];
            println!(" {cam_id}: {count_per_cam}");
        }
        println!("by n points:");
        for (npt, count_per_num_pts) in by_n_pts.iter() {
            println!(" {npt}: {count_per_num_pts}");
        }

        let id_mat = DatMat::new(count, num_cameras, id_mat)?.transpose();
        let points = DatMat::new(count, num_cameras * 3, points)?.transpose();
        (id_mat, points)
    };

    if id_mat.cols == 0 {
        eyre::bail!("No points detected.");
    }

    let undo_radial = radfiles.len() == num_cameras;

    let cfg = McscCfg {
        num_cameras,
        undo_radial,
        use_nth_observation,
    };

    let res = DatMat::new(num_cameras, 2, res)?;

    let mcsc_data = McscConfigDir {
        id_mat,
        radfiles,
        cfg,
        camera_order,
        res,
        points,
    };

    #[allow(unused_variables)]
    let mut output_root_guard = None; // will cleanup on drop

    let input_str = opt
        .input
        .as_os_str()
        .to_str()
        .ok_or_else(|| eyre::eyre!("input filename is not valid unicode?"))?;
    let input_base_name = input_str
        .strip_suffix(".braidz")
        .ok_or_else(|| eyre::eyre!("expected input filename to end with '.braidz'."))?;
    let out_dir_name = if opt.keep {
        PathBuf::from(format!("{}.mcsc", input_base_name))
    } else {
        let output_root = tempfile::tempdir()?;
        let out_dir_name = PathBuf::from(output_root.path());
        #[allow(unused_assignments)]
        {
            output_root_guard = Some(output_root);
        }
        out_dir_name
    };
    let xml_out_name = PathBuf::from(format!("{}-unaligned.xml", input_base_name));

    mcsc_data.save_to_path(&out_dir_name)?;

    println!("Saved to directory \"{}\".", out_dir_name.display());

    if std::fs::exists(&xml_out_name)? {
        eyre::bail!(
            "XML calibration output file (\"{}\") exists. Will not overwrite.",
            xml_out_name.display()
        );
    }

    // unpack MCSC into tempdir
    let mcsc_root = tempfile::tempdir()?;
    let mcsc_dir_name = PathBuf::from(mcsc_root.path());

    let mcsc_base = mcsc_structs::unpack_mcsc_into(&mcsc_dir_name)?;
    let gocal_abs = mcsc_base.join("MultiCamSelfCal/gocal.m");

    let resultdir = out_dir_name.join("result");
    copy_dir_all(&out_dir_name, &resultdir)?;

    let config_arg = format!(
        "--config={resultdir}",
        resultdir = std::path::absolute(&resultdir)?.display()
    );
    let args = vec![gocal_abs.as_os_str(), config_arg.as_ref()];
    let current_dir = gocal_abs.parent().unwrap();
    if !std::process::Command::new("octave")
        .args(args)
        .current_dir(current_dir)
        .status()?
        .success()
    {
        eyre::bail!("octave failed");
    }

    println!("Reading calibration at {}", resultdir.display());

    let calibration = FlydraMultiCameraSystem::<f64>::from_path(&resultdir)
        .with_context(|| format!("while reading calibration at {}", resultdir.display()))?;

    let mut out_fd = std::fs::File::create_new(&xml_out_name).with_context(|| {
        format!(
            "While creating XML calibration output file {}",
            xml_out_name.display()
        )
    })?;
    calibration.to_flydra_xml(&mut out_fd)?;

    Ok(xml_out_name)
}

#[cfg(test)]
mod test {
    use ::zip::ZipArchive;
    use eyre::Result;
    use std::io::Seek;

    use super::*;

    const FNAME: &str = "braidz-mcsc-cal-test-data.zip";
    const URL_BASE: &str = "https://strawlab-cdn.com/assets/";
    const SHA256SUM: &str = "f0043d73749e9c2c161240436eca9101a4bf71cf81785a45b04877fe7ae6d33e";

    fn unpack_zip_into<R: Read + Seek>(
        mut archive: ZipArchive<R>,
        mcsc_dir_name: &Path,
    ) -> Result<()> {
        fs::create_dir_all(&mcsc_dir_name).unwrap();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).unwrap();
            let outpath = match file.enclosed_name() {
                Some(path) => path.to_owned(),
                None => continue,
            };
            let outpath = mcsc_dir_name.join(outpath);

            if (*file.name()).ends_with('/') {
                fs::create_dir_all(&outpath).unwrap();
            } else {
                if let Some(p) = outpath.parent() {
                    if !p.exists() {
                        fs::create_dir_all(p).unwrap();
                    }
                }
                let mut outfile = fs::File::create(&outpath).unwrap();
                io::copy(&mut file, &mut outfile).unwrap();
            }
        }
        Ok(())
    }

    #[test]
    #[ignore] // Ignore normally because it is slow and requires Octave.
    fn test_braiz_mcsc() -> Result<()> {
        download_verify::download_verify(
            format!("{}/{}", URL_BASE, FNAME).as_str(),
            FNAME,
            &download_verify::Hash::Sha256(SHA256SUM.into()),
        )
        .unwrap();

        let data_root = tempfile::tempdir()?;
        let data_root_dir_name = PathBuf::from(data_root.path());

        let rdr = std::fs::File::open(FNAME)?;
        let cal_data_archive = ZipArchive::new(rdr)?;

        unpack_zip_into(cal_data_archive, &data_root_dir_name)?;

        let input = data_root_dir_name.join("20241017_164418.braidz");
        let checkerboard_cal_dir = Some(data_root_dir_name.join("checkerboard-cal-results"));

        let opt = Cli {
            input,
            checkerboard_cal_dir,
            ..Default::default()
        };
        let _xml_out_name = braiz_mcsc(opt)?;
        // TODO: check that the calibration makes sense...
        Ok(())
    }
}
