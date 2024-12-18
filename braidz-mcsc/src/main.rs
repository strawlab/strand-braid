use clap::Parser;
use eyre::{self, Context, Result};
use polars::prelude::*;
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use flydra_mvg::FlydraMultiCameraSystem;

#[derive(Parser)]
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

    /// Location of the "gocal.m" script to run with octave.
    #[arg(long)]
    gocal: Option<PathBuf>,

    /// If set, keep the intermediate MCSC calibration directory.
    #[arg(long)]
    keep: bool,
}

/// All things saved to an MCSC directory
struct McscConfigDir {
    id_mat: DatMat<i8>,
    res: DatMat<usize>,
    radfiles: Vec<RadFile>,
    camera_order: Vec<String>,
    cfg: McscCfg,
    points: DatMat<f64>,
}

struct DatMat<T> {
    rows: usize,
    cols: usize,
    vals: Vec<T>,
}

impl<T> DatMat<T>
where
    T: Copy,
{
    fn new(rows: usize, cols: usize, vals: Vec<T>) -> Result<Self> {
        if vals.len() != rows * cols {
            eyre::bail!("wrong size");
        }
        Ok(Self { rows, cols, vals })
    }
}

impl<T> DatMat<T>
where
    T: Copy,
{
    fn transpose(&self) -> Self {
        let mut vals = Vec::with_capacity(self.vals.len());
        for col in 0..self.cols {
            for row in 0..self.rows {
                vals.push(self.vals[row * self.cols + col]);
            }
        }
        Self {
            rows: self.cols,
            cols: self.rows,
            vals,
        }
    }
}

impl<T> DatMat<T>
where
    T: std::fmt::Display,
{
    fn save<P: AsRef<Path>>(&self, p: P) -> Result<()> {
        let mut fd = std::fs::File::create(p)?;
        for row in 0..self.rows {
            let row_vals = &self.vals[row * self.cols..(row + 1) * self.cols];
            let row_str: Vec<String> = row_vals.iter().map(ToString::to_string).collect();
            fd.write_all(row_str.join(" ").as_bytes())?;
            fd.write_all(b"\n")?;
        }
        Ok(())
    }
}

#[test]
fn test_transpose() {
    /*
    1, 2, 3
    4, 5, 6

    ->

    1, 4
    2, 5
    3, 6
     */
    let a = DatMat::new(2, 3, vec![1, 2, 3, 4, 5, 6]).unwrap();
    let b = a.transpose();
    assert_eq!(b.rows, 3);
    assert_eq!(b.cols, 2);
    assert_eq!(b.vals, vec![1, 4, 2, 5, 3, 6]);
}

struct RadFile {
    /// linear intrinsics stored in row-major form
    k: Vec<f64>,
    distortion: Vec<f64>,
}
impl RadFile {
    fn new(intrinsics: &opencv_ros_camera::RosCameraInfo<f64>) -> Result<Self> {
        let k = intrinsics.camera_matrix.data.clone();
        if k.len() != 9 {
            eyre::bail!("expected exactly 9 values in camera matrix");
        }
        let distortion = intrinsics.distortion_coefficients.data.clone();
        if distortion.len() > 4 {
            for val in &distortion[4..] {
                if *val != 0.0 {
                    eyre::bail!(
                        "found non-zero high order distortion term which cannot be represented"
                    );
                }
            }
        }
        let distortion = (&distortion[..4]).to_vec();
        assert_eq!(distortion.len(), 4);
        Ok(Self { k, distortion })
    }
    fn save<P: AsRef<Path>>(&self, p: P) -> Result<()> {
        let mut fd = std::fs::File::create(p)?;
        for row in 0..3 {
            for col in 0..3 {
                let val = self.k[row * 3 + col];
                fd.write_all(format!("K{}{} = {}\n", row + 1, col + 1, val).as_bytes())?;
            }
        }
        fd.write_all(b"\n")?;

        for i in 0..4 {
            let val = self.distortion[i];
            fd.write_all(format!("kc{} = {}\n", i + 1, val).as_bytes())?;
        }

        Ok(())
    }
}

struct McscCfg {
    num_cameras: usize,
    undo_radial: bool,
    use_nth_observation: u16,
}

impl McscCfg {
    fn save<P: AsRef<Path>>(&self, p: P) -> Result<()> {
        let mut fd = std::fs::File::create(p)?;
        fd.write_all(
            format!(
                "[Files]
Basename: basename
Image-Extension: jpg

[Images]
Subpix: 0.5

[Calibration]
Num-Cameras: {num_cameras}
Num-Projectors: 0
Nonlinear-Parameters: 0    0    0    0    0    0
Nonlinear-Update: 0   0   0   0   0   0
Do-Global-Iterations: 0
Num-Cameras-Fill: {num_cameras}
Undo-Radial: {undo_radial}
Use-Nth-Frame: {use_nth_observation}
",
                num_cameras = self.num_cameras,
                undo_radial = self.undo_radial as i8,
                use_nth_observation = self.use_nth_observation,
            )
            .as_bytes(),
        )?;

        Ok(())
    }
}

impl McscConfigDir {
    fn save_to_path<P: AsRef<Path>>(&self, p: P) -> Result<()> {
        let base = PathBuf::from(p.as_ref());
        std::fs::create_dir_all(&base)?;

        self.id_mat.save(&base.join("IdMat.dat"))?;
        self.res.save(&base.join("Res.dat"))?;

        for (i, radfile) in self.radfiles.iter().enumerate() {
            let fname = base.join(format!("basename{}.rad", i + 1));
            radfile.save(&fname)?;
        }

        {
            let camera_order_fname = base.join("camera_order.txt");
            let mut fd = std::fs::File::create(camera_order_fname)?;
            for cam in self.camera_order.iter() {
                fd.write_all(format!("{cam}\n").as_bytes())?;
            }
        }

        self.cfg.save(&base.join("multicamselfcal.cfg"))?;

        self.points.save(&base.join("points.dat"))?;

        Ok(())
    }
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
        /*

        851 points
        by camera id:
         Basler_40022057: 802
         Basler_40025037: 816
         Basler_40025042: 657
         Basler_40025383: 846
        by n points:
         3: 283
         4: 568

        */

        let id_mat = DatMat::new(count, num_cameras, id_mat)?.transpose();
        let points = DatMat::new(count, num_cameras * 3, points)?.transpose();
        (id_mat, points)
    };

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

    let out_dir_name = if opt.keep {
        PathBuf::from(format!("{}.mcsc", opt.input.display()))
    } else {
        let output_root = tempfile::tempdir()?;
        let out_dir_name = PathBuf::from(output_root.path());
        #[allow(unused_assignments)]
        {
            output_root_guard = Some(output_root);
        }
        out_dir_name
    };
    let xml_out_name = PathBuf::from(format!("{}.xml", opt.input.display()));

    mcsc_data.save_to_path(&out_dir_name)?;

    println!("Saved to directory \"{}\".", out_dir_name.display());

    if std::fs::exists(&xml_out_name)? {
        eyre::bail!(
            "XML calibration output file (\"{}\") exists. Will not overwrite.",
            xml_out_name.display()
        );
    }

    if let Some(gocal) = &opt.gocal {
        let resultdir = out_dir_name.join("result");
        copy_dir_all(&out_dir_name, &resultdir)?;

        let config_arg = format!(
            "--config={resultdir}",
            resultdir = std::path::absolute(&resultdir)?.display()
        );
        let args = vec![gocal.as_os_str(), config_arg.as_ref()];
        let gocal_abs = std::path::absolute(gocal)?;
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

        println!("Calibration XML saved to {}", xml_out_name.display());
    }

    Ok(())
}
