use std::collections::{BTreeMap, BTreeSet};

use opencv_ros_camera::NamedIntrinsicParameters;
use serde::{Deserialize, Serialize};

use nalgebra::{
    geometry::{Point2, Point3},
    RealField, Vector5,
};

use argmin::core::{CostFunction, Error as ArgminError};

use apriltag_detection_writer::AprilConfig;
use braid_apriltag_types::AprilTagCoords2D;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AprilTagCorrespondingPoint<R: RealField> {
    pub id: i32,
    /// the location of the point in 3D world coordinates
    pub object_point: [R; 3],
    /// the location of the point in 2D pixel coordinates
    pub image_point: [R; 2],
}

impl<R: RealField> From<AprilTagCorrespondingPoint<R>> for dlt::CorrespondingPoint<R> {
    fn from(val: AprilTagCorrespondingPoint<R>) -> Self {
        dlt::CorrespondingPoint {
            object_point: val.object_point,
            image_point: val.image_point,
        }
    }
}

fn cam_with_params(
    orig: &braid_mvg::Camera<f64>,
    param: &[f64],
) -> Result<braid_mvg::Camera<f64>, MyError> {
    let this_distortion = opencv_ros_camera::Distortion::from_opencv_vec(Vector5::new(
        param[0], param[1], param[2], param[3], 0.0,
    ));
    let mut this_intrinsics = orig.intrinsics().clone();
    this_intrinsics.distortion = this_distortion;

    let this_cam = braid_mvg::Camera::new(
        orig.width(),
        orig.height(),
        orig.extrinsics().clone(),
        this_intrinsics,
    )?;
    Ok(this_cam)
}

struct CalibProblem<'a> {
    linear_cam: braid_mvg::Camera<f64>,
    points: &'a [AprilTagCorrespondingPoint<f64>],
}

impl<'a> CostFunction for CalibProblem<'a> {
    type Param = Vec<f64>;
    type Output = f64;

    fn cost(&self, param: &Self::Param) -> Result<Self::Output, ArgminError> {
        let this_cam = cam_with_params(&self.linear_cam, param).unwrap();
        let mean_dist = compute_mean_reproj_dist(&this_cam, self.points);
        Ok(mean_dist)
        // Ok(self.my_cost(&p).unwrap())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MyError {
    pub cam_name: Option<String>,
    pub msg: String,
}

impl MyError {
    pub fn new(msg: String) -> Self {
        Self {
            cam_name: None,
            msg,
        }
    }
}

impl std::error::Error for MyError {}

impl From<std::io::Error> for MyError {
    fn from(orig: std::io::Error) -> MyError {
        MyError::new(format!("std::io::Error: {}", orig))
    }
}

impl From<serde_yaml::Error> for MyError {
    fn from(orig: serde_yaml::Error) -> MyError {
        MyError::new(format!("serde_yaml::Error: {}", orig))
    }
}

impl From<serde_json::Error> for MyError {
    fn from(orig: serde_json::Error) -> MyError {
        MyError::new(format!("serde_json::Error: {}", orig))
    }
}

impl From<braid_mvg::MvgError> for MyError {
    fn from(orig: braid_mvg::MvgError) -> MyError {
        MyError::new(format!("braid_mvg::MvgError: {}", orig))
    }
}

impl From<std::string::FromUtf8Error> for MyError {
    fn from(orig: std::string::FromUtf8Error) -> MyError {
        MyError::new(format!("FromUtf8Error: {}", orig))
    }
}

impl std::fmt::Display for MyError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fiducial3DCoords {
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub fn get_apriltag_cfg<R: std::io::Read>(rdr: R) -> Result<AprilConfig, MyError> {
    use std::io::BufRead;
    let buf_reader = std::io::BufReader::new(rdr);

    enum ReaderState {
        JustStarted,
        InYaml(Vec<String>),
    }

    let mut state = ReaderState::JustStarted;
    for line in buf_reader.lines() {
        let line = line?;
        match state {
            ReaderState::JustStarted => {
                if !line.starts_with("# ") {
                    return Err(MyError::new("No YAML config at start of file".into()));
                }
                if line == "# -- start of yaml config --" {
                    state = ReaderState::InYaml(Vec::new());
                }
            }
            ReaderState::InYaml(ref mut yaml_lines) =>
            {
                #[allow(clippy::manual_strip)]
                if line.starts_with("# ") {
                    if line == "# -- end of yaml config --" {
                        break;
                    } else {
                        let cleaned: &str = &line[2..];
                        yaml_lines.push(cleaned.to_string());
                    }
                } else {
                    return Err(MyError::new(
                        "YAML section started but never finished".into(),
                    ));
                }
            }
        }
    }
    if let ReaderState::InYaml(yaml_lines) = state {
        let mut yaml_buf: Vec<u8> = Vec::new();
        for line in yaml_lines {
            yaml_buf.extend(line.as_bytes());
            yaml_buf.push(b'\n');
        }
        let cfg: AprilConfig = serde_yaml::from_reader(yaml_buf.as_slice())?;
        Ok(cfg)
    } else {
        Err(MyError::new(
            "YAML section started but never finished".into(),
        ))
    }
}

pub struct CalData {
    pub fiducial_3d_coords: Vec<Fiducial3DCoords>,
    pub per_camera_2d: BTreeMap<String, (AprilConfig, Vec<AprilTagCoords2D>)>,
    pub known_good_intrinsics: Option<BTreeMap<String, NamedIntrinsicParameters<f64>>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CalibrationResult {
    pub cam_system: braid_mvg::MultiCameraSystem<f64>,
    pub mean_reproj_dist: BTreeMap<String, f64>,
    pub points: BTreeMap<String, Vec<AprilTagCorrespondingPoint<f64>>>,
}

impl CalibrationResult {
    pub fn to_flydra_xml(&self) -> Result<String, MyError> {
        let flydra_cal =
            flydra_mvg::FlydraMultiCameraSystem::<f64>::from_system(self.cam_system.clone(), None);

        let mut xml_buf: Vec<u8> = Vec::new();
        flydra_cal
            .to_flydra_xml(&mut xml_buf)
            .expect("to_flydra_xml");
        let xml_str = String::from_utf8(xml_buf)?;
        Ok(xml_str)
    }

    pub fn to_pymvg_json(&self) -> Result<Vec<u8>, MyError> {
        let sys = self.cam_system.to_pymvg().unwrap();
        Ok(serde_json::to_vec_pretty(&sys)?)
    }
}

#[derive(Default, Debug)]
struct NoCorresp {
    #[allow(dead_code)]
    ids_3d: BTreeSet<u32>,
    ids_2d: BTreeSet<i32>,
}

fn gather_points_per_cam(
    object_points: &BTreeMap<u32, [f64; 3]>,
    cam_data: &[braid_apriltag_types::AprilTagCoords2D],
) -> Result<Vec<AprilTagCorrespondingPoint<f64>>, NoCorresp> {
    let ids_3d = object_points.keys().map(Clone::clone).collect();
    let mut err = NoCorresp {
        ids_3d,
        ids_2d: Default::default(),
    };

    // Iterate through all rows of detection data to collect all detections
    // per marker.
    let mut uv_per_id = BTreeMap::new();
    for row in cam_data {
        uv_per_id
            .entry(row.id as u32)
            .or_insert_with(Vec::new)
            .push((row.x, row.y)); // The (x,y) pixel coord of detection.
        err.ids_2d.insert(row.id);
    }

    let mut points = Vec::new();
    for (id, uv) in uv_per_id.iter() {
        // calculate mean (u,v) position
        let (sumu, sumv) = uv.iter().fold((0.0, 0.0), |accum, elem| {
            (accum.0 + elem.0, accum.1 + elem.1)
        });
        let u = sumu / uv.len() as f64;
        let v = sumv / uv.len() as f64;

        if let Some(from_csv) = object_points.get(id) {
            let object_point = *from_csv;
            let pt = AprilTagCorrespondingPoint {
                id: (*id).try_into().unwrap(),
                object_point,
                image_point: [u, v],
            };
            points.push(pt);
        }
    }

    if !points.is_empty() {
        Ok(points)
    } else {
        Err(err)
    }
}

struct CamSolution<'a> {
    final_cam: braid_mvg::Camera<f64>,
    points: &'a [AprilTagCorrespondingPoint<f64>],
}

fn dlt(
    cfg: &AprilConfig,
    points: &[dlt::CorrespondingPoint<f64>],
) -> Result<braid_mvg::Camera<f64>, MyError> {
    // Compute linear calibration here
    let epsilon = 1e-10;
    let dlt_pmat =
        dlt::dlt_corresponding(points, epsilon).map_err(|msg| MyError::new(msg.into()))?;

    let cam1 =
        braid_mvg::Camera::from_pmat(cfg.camera_width_pixels, cfg.camera_height_pixels, &dlt_pmat)?;
    let cam2 = cam1.flip().expect("flip camera");

    // take whichever camera points towards objects
    let linear_cam = if mean_forward(&cam1, points) > mean_forward(&cam2, points) {
        cam1
    } else {
        cam2
    };
    Ok(linear_cam)
}

/// put point in image coordinates into object coordinates
///
/// This assumes that the intrinsic parameter matrix is exactly
/// [ fx,  0, cx]
/// [  0, fy, cy]
/// [  0,  0,  1]
///
/// and that the distortion center is at (cx,cy).
fn normalize_point(
    intrinsics: &opencv_ros_camera::RosOpenCvIntrinsics<f64>,
    distorted: (f64, f64),
) -> (f64, f64) {
    let distorted =
        cam_geom::Pixels::new(nalgebra::Vector2::new(distorted.0, distorted.1).transpose());
    let undistorted = intrinsics.undistort(&distorted);
    let x = undistorted.data[(0, 0)];
    let y = undistorted.data[(0, 1)];
    let x = x - intrinsics.cx();
    let y = y - intrinsics.cy();
    let x = x / intrinsics.fx();
    let y = y / intrinsics.fy();
    (x, y)
}

fn run_sqpnp<'a>(
    points: &'a [AprilTagCorrespondingPoint<f64>],
    intrinsics: &NamedIntrinsicParameters<f64>,
) -> Result<CamSolution<'a>, MyError> {
    use glam::f32::{Vec2, Vec3};

    let p2ds: Vec<(f64, f64)> = points
        .iter()
        .map(|p| normalize_point(&intrinsics.intrinsics, (p.image_point[0], p.image_point[1])))
        .collect();
    let p2ds: Vec<Vec2> = p2ds
        .iter()
        .map(|p| Vec2::new(p.0 as f32, p.1 as f32))
        .collect();
    let p3ds: Vec<(f64, f64, f64)> = points
        .iter()
        .map(|p| (p.object_point[0], p.object_point[1], p.object_point[2]))
        .collect();
    let p3ds: Vec<Vec3> = p3ds
        .iter()
        .map(|p| Vec3::new(p.0 as f32, p.1 as f32, p.2 as f32))
        .collect();

    let mut solver = sqpnp::Solver::<sqpnp::DefaultParameters>::new();
    if solver.solve(&p3ds, &p2ds, None) {
        let solution = solver.best_solution().unwrap();
        let r = solution.rotation_matrix();
        // Convert from glam 0.29 (nalgebra 0.33) to nalgebra 0.34. Waiting for
        // https://github.com/ricky26/sqpnp-rs/pull/2
        let r = {
            let row_data = r.as_dmat3().transpose().to_cols_array();
            nalgebra::SMatrix::<f64, 3, 3>::from_row_slice(&row_data)
        };
        // let r: nalgebra::SMatrix<f64, 3, 3> = r.as_dmat3().into();
        let t = solution.translation();

        let extrin = {
            let rotation = nalgebra::UnitQuaternion::from_rotation_matrix(
                &nalgebra::Rotation3::from_matrix(&r),
            );
            let translation = nalgebra::Translation::from(nalgebra::Vector3::new(
                t.x as f64, t.y as f64, t.z as f64,
            ));
            let transform = nalgebra::Isometry3 {
                translation,
                rotation,
            };
            cam_geom::ExtrinsicParameters::from_pose(&transform)
        };

        let final_cam = braid_mvg::Camera::new(
            intrinsics.width,
            intrinsics.height,
            extrin,
            intrinsics.intrinsics.clone(),
        )
        .unwrap();

        Ok(CamSolution { final_cam, points })
    } else {
        Err(MyError::new("sqpnp failed to find solution".to_string()))
    }
}

fn dlt_then_distortion<'a>(
    cfg: &AprilConfig,
    points: &'a [AprilTagCorrespondingPoint<f64>],
) -> Result<CamSolution<'a>, MyError> {
    let dlt_points: Vec<_> = points.iter().map(|x| x.clone().into()).collect();
    // First, calculate "linear" (no distortion) camera model using DLT.
    let linear_cam = dlt(cfg, &dlt_points)?;

    // Second, refine the model with distortion using iterative reduction of
    // mean reprojection distances.

    // Create cost function for optimization of distortion terms.
    let problem = CalibProblem { linear_cam, points };
    use argmin::solver::neldermead::NelderMead;

    let params: Vec<Vec<f64>> = vec![
        vec![-1.0, -1.0, -1.0, -1.0],
        vec![1.0, -1.0, -1.0, -1.0],
        vec![1.0, 1.0, -1.0, -1.0],
        vec![1.0, 1.0, 1.0, -1.0],
        vec![1.0, 1.0, 1.0, 1.0],
    ];

    let nm: NelderMead<_, f64> = NelderMead::new(params);

    let res = argmin::core::Executor::new(problem, nm)
        .configure(|state| {
            state
                // .param(init_param)
                // .inv_hessian(init_hessian)
                .max_iters(1000)
        })
        .run()
        .unwrap();

    let problem = res.problem;
    let CalibProblem { linear_cam, points } = problem.problem.unwrap();

    let final_cam = cam_with_params(&linear_cam, res.state.best_param.unwrap().as_slice())?;

    Ok(CamSolution { final_cam, points })
}

pub fn run_sqpnp_or_dlt(src_data: &CalData) -> Result<CalibrationResult, MyError> {
    let mut object_points = BTreeMap::new();
    for row in src_data.fiducial_3d_coords.iter() {
        if object_points
            .insert(row.id, [row.x, row.y, row.z])
            .is_some()
        {
            return Err(MyError::new(format!(
                "multiple entries for ID {} in 3D data file",
                row.id
            )));
        }
    }

    let mut mean_reproj_dist = BTreeMap::new();
    let mut cams = BTreeMap::new();
    let mut cam_points = BTreeMap::new();

    if src_data.per_camera_2d.is_empty() {
        return Err(MyError::new("No camera has 2D detections.".to_string()));
    }

    for (cam_name, all_cam_data) in src_data.per_camera_2d.iter() {
        let (cfg, cam_data) = all_cam_data;
        assert_eq!(&cfg.camera_name, cam_name);

        let points = match gather_points_per_cam(&object_points, cam_data) {
            Ok(points) => points,
            Err(err) => {
                return Err(MyError{cam_name: Some(cam_name.clone()), msg:format!("Camera {cam_name}: no matching April Tags in 3D coords and 2D detections {err:?}")});
            }
        };

        let sln = if let Some(kgi) = src_data.known_good_intrinsics.as_ref() {
            if points.len() < 4 {
                return Err(MyError {
                    cam_name: Some(cam_name.clone()),
                    msg:
                        format!("For camera \"{cam_name}\": Need minimum 4 corresponding 3D and 2D points to run SQPnP."),
                });
            }
            let known_good_intrinsics = kgi.get(cam_name).unwrap();
            match run_sqpnp(&points, known_good_intrinsics) {
                Ok(sln) => sln,
                Err(my_error) => {
                    let corr_ids: Vec<_> = points.iter().map(|x| x.id).collect();
                    tracing::info!(
                        "for camera \"{cam_name}\": 3d and 2d corresponding point ids: {corr_ids:?}"
                    );
                    tracing::info!(
                        "for camera \"{cam_name}\": fx: {:.1}, fy: {:.1}, cx: {:.1}, cy: {:.1}, distortion: {:?}",
                        known_good_intrinsics.intrinsics.fx(),
                        known_good_intrinsics.intrinsics.fy(),
                        known_good_intrinsics.intrinsics.cx(),
                        known_good_intrinsics.intrinsics.cy(),
                        known_good_intrinsics.intrinsics.distortion,
                    );
                    return Err(MyError {
                        cam_name: Some(cam_name.clone()),
                        msg: format!(
                            "While running run_sqpnp for camera \"{cam_name}\": {}. Check input 3d points, 2d points, and intrinsics.",
                            my_error.msg
                        ),
                    });
                }
            }
        } else {
            dlt_then_distortion(cfg, &points)?
        };

        let CamSolution { final_cam, points } = sln;
        let mean_dist = compute_mean_reproj_dist(&final_cam, points);

        cams.insert(cam_name.clone(), final_cam);
        mean_reproj_dist.insert(cam_name.clone(), mean_dist);
        cam_points.insert(cam_name.clone(), points.to_vec());
    }

    let cam_system = braid_mvg::MultiCameraSystem::new(cams);

    Ok(CalibrationResult {
        cam_system,
        mean_reproj_dist,
        points: cam_points,
    })
}

/// Compute reprojection distance.
pub fn compute_mean_reproj_dist(
    cam: &braid_mvg::Camera<f64>,
    points: &[AprilTagCorrespondingPoint<f64>],
) -> f64 {
    assert!(!points.is_empty());

    // Compute reprojection distance.
    let dists: Vec<f64> = points
        .iter()
        .map(|pt| {
            let world_pt = braid_mvg::PointWorldFrame {
                coords: Point3::from_slice(&pt.object_point),
            };
            let image_point = Point2::from_slice(&pt.image_point);
            let projected_pixel = cam.project_3d_to_distorted_pixel(&world_pt);
            nalgebra::distance(&projected_pixel.coords, &image_point)
        })
        .collect();

    let sum_dist = dists.iter().fold(0.0, |accum, el| accum + el);
    sum_dist / dists.len() as f64
}

fn mean_forward(cam: &braid_mvg::Camera<f64>, pts: &[dlt::CorrespondingPoint<f64>]) -> f64 {
    use braid_mvg::PointWorldFrame;
    let mut accum = 0.0;
    for pt in pts {
        let o = pt.object_point;
        let world_pt = PointWorldFrame {
            coords: Point3::from_slice(&o),
        };

        let wc2b: cam_geom::Points<_, _, nalgebra::U1, _> = (&world_pt).into();
        let cam_pt = cam.extrinsics().world_to_camera(&wc2b);
        let cam_dist = cam_pt.data[(0, 2)];
        accum += cam_dist;
    }
    accum / pts.len() as f64
}
