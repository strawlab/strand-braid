use clap::Parser;
use color_eyre::{
    eyre::{bail, Context},
    Result,
};
use flydra_mvg::FlydraMultiCameraSystem;
use mvg::align_points::{align_points, Algorithm};
use nalgebra::{Dyn, OMatrix, U1, U3};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "cal-to-xml", version)]
struct Opt {
    /// Filename of .csv file containing reference (ground truth) 3D positions
    #[arg(long)]
    ground_truth_3d: std::path::PathBuf,

    /// Filename of .csv file containing unaligned 3D positions
    #[arg(long)]
    unaligned_3d: std::path::PathBuf,

    /// Filename of .xml file containing unaligned calibration
    #[arg(long)]
    unaligned_cal: std::path::PathBuf,

    /// Filename of .xml file containing output aligned calibration
    #[arg(long)]
    output_aligned_cal: Option<std::path::PathBuf>,
}

#[derive(Deserialize, Serialize, Debug)]
struct Xyz {
    x: f64,
    y: f64,
    z: f64,
}

impl Xyz {
    fn iter(&self) -> impl Iterator<Item = f64> {
        vec![self.x, self.y, self.z].into_iter()
    }
}

fn to_arr(vals: &[Xyz]) -> Result<OMatrix<f64, U3, Dyn>> {
    let n = vals.len();
    let mut result = OMatrix::<f64, U3, Dyn>::zeros(n);
    for (j, xyz) in vals.iter().enumerate() {
        for (i, val) in xyz.iter().enumerate() {
            if val.is_nan() {
                bail!("Entry ({i},{j}) is NAN");
            }
            result[(i, j)] = val;
        }
    }
    Ok(result)
}

fn main() -> Result<()> {
    let opt = Opt::parse();

    let ground_truth_3d_fd = std::fs::File::open(&opt.ground_truth_3d)
        .with_context(|| format!("While opening {}", opt.ground_truth_3d.display()))?;

    let unaligned_3d_fd = std::fs::File::open(&opt.unaligned_3d)
        .with_context(|| format!("While opening {}", opt.unaligned_3d.display()))?;

    let unaligned_calibration = FlydraMultiCameraSystem::<f64>::from_path(&opt.unaligned_cal)
        .with_context(|| {
            format!(
                "while reading calibration at {}",
                opt.unaligned_cal.display()
            )
        })?;

    let output_aligned_cal = if let Some(path) = opt.output_aligned_cal {
        path
    } else {
        let mut path = opt.unaligned_cal.clone();
        path.set_extension("");
        PathBuf::from(format!("{}-aligned.xml", path.display()))
    };

    let ground_truth_3d_rdr = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .from_reader(ground_truth_3d_fd);
    let mut ground_truth_3d_rows = Vec::new();
    for row in ground_truth_3d_rdr.into_deserialize() {
        ground_truth_3d_rows.push(row?);
    }

    let unaligned_3d_rdr = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .from_reader(unaligned_3d_fd);
    let mut unaligned_3d_rows = Vec::new();
    for row in unaligned_3d_rdr.into_deserialize() {
        unaligned_3d_rows.push(row?);
    }

    if unaligned_3d_rows.len() != ground_truth_3d_rows.len() {
        bail!("unaligned_3d and ground_truth_3d do not contain exactly the same number of points");
    }

    let x1 = to_arr(&unaligned_3d_rows)?;
    let x2 = to_arr(&ground_truth_3d_rows)?;

    let (s, rot, t) = align_points(&x1, &x2, Algorithm::RobustArun)?;

    println!("Found alignment transform: -------");
    println!("scale: {s}");
    println!("rotation:{rot}");
    println!("translation:{t}");

    let xformed = s * &rot * &x1 + bcast(&t, unaligned_3d_rows.len());

    print!("Original unaligned points: -------");
    println!("{x1}");

    print!("Ground truth points: -------");
    println!("{x2}");

    print!("Transformed points: -------");
    println!("{xformed}");

    println!("Distances between ground truth and transformed points: -------");
    let d = xformed - x2;
    let d2 = d.component_mul(&d); // square
    let dvec: Vec<f64> = d2.row_sum().iter().map(|val| val.sqrt()).collect();
    println!("{dvec:?}");

    println!("Mean distance between ground truth and transformed points: -------");
    let mean_dist = nalgebra::Vector::<f64, Dyn, _>::from_vec(dvec).mean();
    println!("{mean_dist}");

    let system = unaligned_calibration.system().align(s, rot, t)?;
    let aligned = FlydraMultiCameraSystem::from_system(system, unaligned_calibration.water());

    let mut out_fd = std::fs::File::create_new(&output_aligned_cal).with_context(|| {
        format!(
            "While creating output file {}",
            output_aligned_cal.display()
        )
    })?;
    aligned.to_flydra_xml(&mut out_fd)?;

    Ok(())
}

fn bcast(m: &OMatrix<f64, U3, U1>, n: usize) -> OMatrix<f64, U3, Dyn> {
    // this is far from efficient
    let mut result = OMatrix::<f64, U3, Dyn>::zeros(n);
    for i in 0..3 {
        for j in 0..n {
            result[(i, j)] = m[(i, 0)];
        }
    }
    result
}
