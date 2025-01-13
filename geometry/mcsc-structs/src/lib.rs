use eyre::Result;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub struct DatMat<T> {
    rows: usize,
    pub cols: usize,
    vals: Vec<T>,
}

impl<T> DatMat<T>
where
    T: Copy,
{
    pub fn new(rows: usize, cols: usize, vals: Vec<T>) -> Result<Self> {
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
    pub fn transpose(&self) -> Self {
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

pub struct RadFile {
    /// linear intrinsics stored in row-major form
    k: Vec<f64>,
    distortion: Vec<f64>,
}
impl RadFile {
    pub fn new(intrinsics: &opencv_ros_camera::RosCameraInfo<f64>) -> Result<Self> {
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

/// All things saved to an MCSC directory
pub struct McscConfigDir {
    pub id_mat: DatMat<i8>,
    pub res: DatMat<usize>,
    pub radfiles: Vec<RadFile>,
    pub camera_order: Vec<String>,
    pub cfg: McscCfg,
    pub points: DatMat<f64>,
}

impl McscConfigDir {
    pub fn save_to_path<P: AsRef<Path>>(&self, p: P) -> Result<()> {
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
