use eyre::Result;
use std::{
    fs,
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
};
use zip::ZipArchive;

static MCSC_RELEASE: &[u8] = include_bytes!("../multicamselfcal-0.3.2.zip"); // use package-mcsc-zip.sh
static MCSC_DIRNAME: &str = "multicamselfcal-0.3.2";

#[derive(Clone)]
pub struct DatMat<T> {
    rows: usize,
    cols: usize,
    /// row-major storage
    vals: Vec<T>,
}

impl<T> DatMat<T> {
    pub fn new(rows: usize, cols: usize, vals: Vec<T>) -> Result<Self> {
        if vals.len() != rows * cols {
            eyre::bail!("wrong size");
        }
        Ok(Self { rows, cols, vals })
    }
    pub fn nrows(&self) -> usize {
        self.rows
    }
    pub fn ncols(&self) -> usize {
        self.cols
    }
}

impl<T> std::ops::Index<(usize, usize)> for DatMat<T> {
    type Output = T;

    fn index(&self, index: (usize, usize)) -> &Self::Output {
        let (i, j) = index;
        &self.vals[i * self.cols + j]
    }
}

impl<T> DatMat<T>
where
    T: Clone,
{
    pub fn transpose(&self) -> Self {
        let mut vals = Vec::with_capacity(self.vals.len());
        for col in 0..self.cols {
            for row in 0..self.rows {
                vals.push(self.vals[row * self.cols + col].clone());
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

impl From<DatMat<bool>> for DatMat<i8> {
    fn from(orig: DatMat<bool>) -> Self {
        let vals = orig.vals.into_iter().map(|x| x.into()).collect();
        Self {
            rows: orig.rows,
            cols: orig.cols,
            vals,
        }
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

pub struct RadFile {
    /// linear intrinsics stored in row-major form
    k: Vec<f64>,
    distortion: Vec<f64>,
}
impl RadFile {
    pub fn new(cam_info: &opencv_ros_camera::RosCameraInfo<f64>) -> Result<Self> {
        let k = cam_info.camera_matrix.data.clone();
        if k.len() != 9 {
            eyre::bail!("expected exactly 9 values in camera matrix");
        }
        let distortion = cam_info.distortion_coefficients.data.clone();
        if distortion.len() > 4 {
            for val in &distortion[4..] {
                if *val != 0.0 {
                    eyre::bail!(
                        "found non-zero high order distortion term which cannot be represented"
                    );
                }
            }
        }
        let distortion = distortion[..4].to_vec();
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

/// All things saved to an MCSC directory
pub struct McscConfigDir {
    /// indicates whether point is visible from camera (shape: n_cams x n_points)
    pub id_mat: DatMat<i8>,
    /// pixel size of each camera (shape n_cams x 2)
    pub res: DatMat<usize>,
    pub radfiles: Vec<RadFile>,
    pub camera_order: Vec<String>,
    pub cfg: McscCfg,
    /// image coordinates of point from camera (shape: n_cams*3 x n_points)
    pub points: DatMat<f64>,
}

impl McscConfigDir {
    fn validate(&self) -> Result<()> {
        let n_cams = self.id_mat.rows;
        let n_points = self.id_mat.cols;

        if self.points.rows != n_cams * 3 {
            eyre::bail!("inconsistent number of cameras");
        }
        if self.points.cols != n_points {
            eyre::bail!("inconsistent number of points");
        }

        if self.camera_order.len() != n_cams {
            eyre::bail!("inconsistent number of cameras");
        }

        if self.res.rows != n_cams {
            eyre::bail!("inconsistent number of cameras");
        }

        if self.res.cols != 2 {
            eyre::bail!("inconsistent `res` data");
        }

        Ok(())
    }

    pub fn save_to_path<P: AsRef<Path>>(&self, p: P) -> Result<()> {
        self.validate()?;

        let base = PathBuf::from(p.as_ref());
        std::fs::create_dir_all(&base)?;

        self.id_mat.save(base.join("IdMat.dat"))?;
        self.res.save(base.join("Res.dat"))?;

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

        self.cfg.save(base.join("multicamselfcal.cfg"))?;

        self.points.save(base.join("points.dat"))?;

        Ok(())
    }
}

pub struct McscCfg {
    pub num_cameras: usize,
    pub undo_radial: bool,
    pub use_nth_observation: u16,
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

/// Unpack the mcsc source into the directory specified and return the path into
/// which `MultiCamSelfCal/gocal.m` was saved.
pub fn unpack_mcsc_into(mcsc_dir_name: &Path) -> Result<PathBuf> {
    // open MCSC zip archive
    let rdr = std::io::Cursor::new(MCSC_RELEASE);
    let mcsc_zip_archive = ZipArchive::new(rdr)?;
    // unpack MCSC into tempdir
    unpack_zip_into(mcsc_zip_archive, mcsc_dir_name)?;

    Ok(mcsc_dir_name.join(MCSC_DIRNAME))
}

fn unpack_zip_into<R: Read + Seek>(mut archive: ZipArchive<R>, mcsc_dir_name: &Path) -> Result<()> {
    fs::create_dir_all(mcsc_dir_name).unwrap();
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
            if let Some(p) = outpath.parent()
                && !p.exists() {
                    fs::create_dir_all(p).unwrap();
                }
            let mut outfile = fs::File::create(&outpath).unwrap();
            io::copy(&mut file, &mut outfile).unwrap();
        }
    }
    Ok(())
}
