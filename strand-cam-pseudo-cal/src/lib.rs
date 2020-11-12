extern crate nalgebra as na;

use flydra_types::RawCamName;
pub type MyFloat = f64;

use http_video_streaming_types::CircleParams;

// TODO check KalmanTrackingConfig
// see:
// flydratrax_handle_msg::flydratrax_handle_msg
// PseudoCameraCalibrationData
/// create a camera calibration from a few values
pub struct PseudoCameraCalibrationData {
    pub cam_name: RawCamName,
    pub width: u32,
    pub height: u32,
    pub physical_diameter_meters: f32,
    pub image_circle: CircleParams,
}

impl PseudoCameraCalibrationData {
    pub fn to_cam(&self) -> Result<mvg::Camera<MyFloat>, mvg::MvgError> {
        use na::geometry::{Point3, UnitQuaternion};
        use na::core::Vector3;

        let zdist = 0.1; // Z distance is hardcoded to a fixed value.

        let zpos: f64 = -zdist;
        // camera at 0,0,zpos looking up +Z axis with local up -Y axis
        let extrinsics = {
            let axis = na::core::Unit::new_normalize(Vector3::x());
            let angle = na::convert( 0.0 );
            let rquat = UnitQuaternion::from_axis_angle(&axis, angle);
            let camcenter = Point3::from( Vector3::new( 0.0, 0.0, zpos));
            cam_geom::ExtrinsicParameters::from_rotation_and_camcenter(rquat, camcenter)
        };

        // m*focal_length = radius -> focal_length = radius/m
        // Where m is the 3D camera coordinates of a point. In this
        // case we choose a point at the x axis of the unit circle for
        // simplicity.
        let m: f64 = self.physical_diameter_meters as f64 * 0.5 / zdist;
        let f = self.image_circle.radius as f64/m;

        let f: MyFloat = na::convert(f);
        let cx: MyFloat = na::convert(self.image_circle.center_x);
        let cy: MyFloat = na::convert(self.image_circle.center_y);
        let intrinsics = opencv_ros_camera::RosOpenCvIntrinsics::from_params(f,0.0,f,cx,cy);

        mvg::Camera::new(self.width as usize, self.height as usize,
            extrinsics, intrinsics)
    }

    pub fn to_camera_system(&self) -> Result<flydra_mvg::FlydraMultiCameraSystem<MyFloat>, failure::Error> {
        let cam_name = self.cam_name.clone();
        let cam = self.to_cam()?;
        let mut cams_by_name = std::collections::BTreeMap::new();
        cams_by_name.insert(cam_name.to_ros().as_str().to_string(), cam);

        let data = serde_json::json!({
            "pseudo_camera_calibration": 1,
            "physical_diameter_meters": self.physical_diameter_meters,
        });

        let comment = serde_json::to_string(&data).unwrap();
        let plain_vanilla = mvg::MultiCameraSystem::new_with_comment(cams_by_name, comment);
        let spicy = flydra_mvg::FlydraMultiCameraSystem::from_system(plain_vanilla, None);
        Ok(spicy)
    }
}

#[test]
fn test_pseudo_cal() {
    use mvg::{PointWorldFrame, DistortedPixel};
    use na::geometry::{Point2, Point3};

    let pc = PseudoCameraCalibrationData {
        cam_name: flydra_types::RawCamName::new("pseudo-cam".to_string()),
        width: 640,
        height: 480,
        physical_diameter_meters: 0.03,
        image_circle: CircleParams {
            center_x: 320,
            center_y: 240,
            radius: 100,
        },
    };
    let cam = pc.to_cam().expect("pc.to_cam()");

    let m: f64 = pc.physical_diameter_meters as f64 * 0.5;
    let pt_x = PointWorldFrame { coords: Point3::new(m, 0.0, 0.0) };
    let pt_y = PointWorldFrame { coords: Point3::new(0.0, m, 0.0) };

    let actual_x = cam.project_3d_to_distorted_pixel(&pt_x);
    let actual_y = cam.project_3d_to_distorted_pixel(&pt_y);

    let expected_x = DistortedPixel {
        coords: Point2::new( pc.image_circle.center_x as f64 + pc.image_circle.radius as f64,
            pc.image_circle.center_y as f64)};
    let expected_y = DistortedPixel {
        coords: Point2::new( pc.image_circle.center_x as f64,
        pc.image_circle.center_y as f64 + pc.image_circle.radius as f64)};
    approx::assert_relative_eq!(actual_x.coords, expected_x.coords, max_relative = 1e-5);
    approx::assert_relative_eq!(actual_y.coords, expected_y.coords, max_relative = 1e-5);
}
