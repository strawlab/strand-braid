use std::collections::BTreeMap;

use opencv_ros_camera::NamedIntrinsicParameters;
use serde::{Deserialize, Serialize};

use nalgebra::{
    geometry::{Point2, Point3},
    RealField, Vector5,
};

use argmin::core::{CostFunction, Error as ArgminError};

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

fn cam_with_params(orig: &mvg::Camera<f64>, param: &[f64]) -> Result<mvg::Camera<f64>, MyError> {
    let this_distortion = opencv_ros_camera::Distortion::from_opencv_vec(Vector5::new(
        param[0], param[1], param[2], param[3], 0.0,
    ));
    let mut this_intrinsics = orig.intrinsics().clone();
    this_intrinsics.distortion = this_distortion;

    let this_cam = mvg::Camera::new(
        orig.width(),
        orig.height(),
        orig.extrinsics().clone(),
        this_intrinsics,
    )?;
    Ok(this_cam)
}

struct CalibProblem {
    linear_cam: mvg::Camera<f64>,
    points: Vec<AprilTagCorrespondingPoint<f64>>,
}

impl CostFunction for CalibProblem {
    type Param = Vec<f64>;
    type Output = f64;

    fn cost(&self, param: &Self::Param) -> Result<Self::Output, ArgminError> {
        let this_cam = cam_with_params(&self.linear_cam, param).unwrap();
        let mean_dist = compute_mean_reproj_dist(&this_cam, &self.points);
        Ok(mean_dist)
        // Ok(self.my_cost(&p).unwrap())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MyError {
    pub msg: String,
}

impl std::error::Error for MyError {}

impl From<std::io::Error> for MyError {
    fn from(orig: std::io::Error) -> MyError {
        MyError {
            msg: format!("std::io::Error: {}", orig),
        }
    }
}

impl From<serde_yaml::Error> for MyError {
    fn from(orig: serde_yaml::Error) -> MyError {
        MyError {
            msg: format!("serde_yaml::Error: {}", orig),
        }
    }
}

impl From<serde_json::Error> for MyError {
    fn from(orig: serde_json::Error) -> MyError {
        MyError {
            msg: format!("serde_json::Error: {}", orig),
        }
    }
}

impl From<mvg::MvgError> for MyError {
    fn from(orig: mvg::MvgError) -> MyError {
        MyError {
            msg: format!("mvg::MvgError: {}", orig),
        }
    }
}

#[cfg(feature = "solve-pnp")]
impl From<opencv_calibrate::Error> for MyError {
    fn from(orig: opencv_calibrate::Error) -> MyError {
        MyError {
            msg: format!("opencv_calibrate::Error: {}", orig),
        }
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

// The center pixel of the detection is (h02,h12)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DetectionSerializer {
    pub id: i32,
    pub h02: f64,
    pub h12: f64,
}

/// This matches the definition in strand-cam.rs. TODO: fix DRY violation.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AprilConfig {
    pub created_at: chrono::DateTime<chrono::Local>,
    pub camera_name: String,
    pub camera_width_pixels: usize,
    pub camera_height_pixels: usize,
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
                    return Err(MyError {
                        msg: "File did not start with comment '# '".into(),
                    });
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
                    return Err(MyError {
                        msg: "YAML section started but never finished".into(),
                    });
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
        Err(MyError {
            msg: "YAML section started but never finished".into(),
        })
    }
}

// #[derive(Serialize, Deserialize)]
pub struct CalData {
    pub fiducial_3d_coords: Vec<Fiducial3DCoords>,
    pub per_camera_2d: BTreeMap<String, (AprilConfig, Vec<DetectionSerializer>)>,
    pub known_good_intrinsics: Option<BTreeMap<String, NamedIntrinsicParameters<f64>>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CalibrationResult {
    pub cam_system: mvg::MultiCameraSystem<f64>,
    pub mean_reproj_dist: BTreeMap<String, f64>,
    pub points: BTreeMap<String, Vec<AprilTagCorrespondingPoint<f64>>>,
}

impl CalibrationResult {
    pub fn to_flydra_xml(&self) -> Result<Vec<u8>, MyError> {
        let flydra_cal =
            flydra_mvg::FlydraMultiCameraSystem::<f64>::from_system(self.cam_system.clone(), None);

        let mut xml_buf: Vec<u8> = Vec::new();
        flydra_cal
            .to_flydra_xml(&mut xml_buf)
            .expect("to_flydra_xml");
        Ok(xml_buf)
    }

    pub fn to_pymvg_json(&self) -> Result<Vec<u8>, MyError> {
        let sys = self.cam_system.to_pymvg().unwrap();
        Ok(serde_json::to_vec_pretty(&sys)?)
    }
}

fn gather_points_per_cam(
    object_points: &BTreeMap<u32, [f64; 3]>,
    cam_data: &[DetectionSerializer],
) -> Result<Vec<AprilTagCorrespondingPoint<f64>>, MyError> {
    // Iterate through all rows of detection data to collect all detections
    // per marker.
    let mut uv_per_id = BTreeMap::new();
    for row in cam_data {
        uv_per_id
            .entry(row.id as u32)
            .or_insert_with(Vec::new)
            .push((row.h02, row.h12)); // The (x,y) pixel coord of detection.
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

    Ok(points)
}

struct CamSolution {
    final_cam: mvg::Camera<f64>,
    points: Vec<AprilTagCorrespondingPoint<f64>>,
}

fn dlt(
    cfg: &AprilConfig,
    points: &[dlt::CorrespondingPoint<f64>],
) -> Result<mvg::Camera<f64>, MyError> {
    // Compute linear calibration here
    let epsilon = 1e-10;
    let dlt_pmat =
        dlt::dlt_corresponding(points, epsilon).map_err(|msg| MyError { msg: msg.into() })?;

    let cam1 =
        mvg::Camera::from_pmat(cfg.camera_width_pixels, cfg.camera_height_pixels, &dlt_pmat)?;
    let cam2 = cam1.flip().expect("flip camera");

    // take whichever camera points towards objects
    let linear_cam = if mean_forward(&cam1, points) > mean_forward(&cam2, points) {
        cam1
    } else {
        cam2
    };
    Ok(linear_cam)
}

#[cfg(feature = "solve-pnp")]
fn solve_extrinsics(
    points: Vec<AprilTagCorrespondingPoint<f64>>,
    intrinsics: &NamedIntrinsicParameters<f64>,
) -> Result<CamSolution, MyError> {
    {
        let cv_points: Vec<opencv_calibrate::CorrespondingPoint> = points
            .iter()
            .map(|pt| {
                let o = &pt.object_point;
                let i = &pt.image_point;
                opencv_calibrate::CorrespondingPoint {
                    object_point: (o[0], o[1], o[2]),
                    image_point: (i[0], i[1]),
                }
            })
            .collect();
        let k = intrinsics.intrinsics.k;
        let cam_matrix = [
            k[(0, 0)],
            k[(0, 1)],
            k[(0, 2)],
            k[(1, 0)],
            k[(1, 1)],
            k[(1, 2)],
            k[(2, 0)],
            k[(2, 1)],
            k[(2, 2)],
        ];
        let dist_coeffs = intrinsics
            .intrinsics
            .distortion
            .opencv_vec()
            .as_slice()
            .try_into()
            .unwrap();
        let cv_extrinsics = opencv_calibrate::solve_pnp(
            &cv_points,
            &cam_matrix,
            &dist_coeffs,
            opencv_calibrate::PoseMethod::Ippe,
        )?;

        let extrin = {
            // convert from OpenCV rodrigues vec to axis-angle
            let [a, b, c] = cv_extrinsics.rvec;
            let angle = (a * a + b * b + c * c).sqrt();
            let axis = nalgebra::Vector3::new(a, b, c);
            let axis = nalgebra::base::Unit::new_normalize(axis);
            let rquat = nalgebra::geometry::UnitQuaternion::from_axis_angle(&axis, angle);
            let rmat = rquat.to_rotation_matrix();

            let [x, y, z] = cv_extrinsics.tvec;
            let t = nalgebra::Point3::new(x, y, z);

            let camcenter = -(rmat.transpose() * t);
            cam_geom::ExtrinsicParameters::from_rotation_and_camcenter(rquat, camcenter)
        };

        let final_cam = mvg::Camera::new(
            intrinsics.width,
            intrinsics.height,
            extrin,
            intrinsics.intrinsics.clone(),
        )
        .unwrap();

        Ok(CamSolution { final_cam, points })
    }
}

fn dlt_then_distortion(
    cfg: &AprilConfig,
    points: Vec<AprilTagCorrespondingPoint<f64>>,
) -> Result<CamSolution, MyError> {
    let dlt_points: Vec<_> = points.clone().into_iter().map(|x| x.into()).collect();
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

pub fn do_calibrate_system(src_data: &CalData) -> Result<CalibrationResult, MyError> {
    let mut object_points = BTreeMap::new();
    for row in src_data.fiducial_3d_coords.iter() {
        if object_points
            .insert(row.id, [row.x, row.y, row.z])
            .is_some()
        {
            return Err(MyError {
                msg: format!("multiple entries for ID {} in 3D data file", row.id),
            });
        }
    }

    let mut mean_reproj_dist = BTreeMap::new();
    let mut cams = BTreeMap::new();
    let mut cam_points = BTreeMap::new();

    for (cam_name, all_cam_data) in src_data.per_camera_2d.iter() {
        let (cfg, cam_data) = all_cam_data;
        assert_eq!(&cfg.camera_name, cam_name);

        let points = gather_points_per_cam(&object_points, cam_data)?;
        if points.is_empty() {
            return Err(MyError{msg:format!("Camera {}: could not compute reprojection distance. Are there marker detections also in 3D data?", cam_name)});
        }

        let sln = if let Some(kgi) = src_data.known_good_intrinsics.as_ref() {
            #[cfg(feature = "solve-pnp")]
            {
                let known_good_intrinsics = kgi.get(cam_name).unwrap();
                solve_extrinsics(points, known_good_intrinsics)?
            }
            #[cfg(not(feature = "solve-pnp"))]
            {
                let _ = kgi;
                return Err(MyError {
                    msg: "'solve-pnp' feature must be enabled to solve extrinsics when intrinsics provided".into(),
                });
            }
        } else {
            dlt_then_distortion(cfg, points)?
        };

        let CamSolution { final_cam, points } = sln;
        let mean_dist = compute_mean_reproj_dist(&final_cam, &points);

        cams.insert(cam_name.clone(), final_cam);
        mean_reproj_dist.insert(cam_name.clone(), mean_dist);
        cam_points.insert(cam_name.clone(), points);
    }

    let cam_system = mvg::MultiCameraSystem::new(cams);

    Ok(CalibrationResult {
        cam_system,
        mean_reproj_dist,
        points: cam_points,
    })
}

/// Compute reprojection distance.
pub fn compute_mean_reproj_dist(
    cam: &mvg::Camera<f64>,
    points: &[AprilTagCorrespondingPoint<f64>],
) -> f64 {
    assert!(!points.is_empty());

    // Compute reprojection distance.
    let dists: Vec<f64> = points
        .iter()
        .map(|pt| {
            let world_pt = mvg::PointWorldFrame {
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

fn mean_forward(cam: &mvg::Camera<f64>, pts: &[dlt::CorrespondingPoint<f64>]) -> f64 {
    use mvg::PointWorldFrame;
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
