use super::*;

/// What parameters are optimized during bundle adjustment.
#[derive(Clone, Debug, PartialEq, Copy, clap::ValueEnum, Default)]
pub enum CameraModelType {
    /// Tunes the 3D world points, the camera extrinsic parameters, and the
    /// camera intrinsic parameters including all 5 distortion terms (3 radial
    /// distortions, 2 tangential distortions) in the OpenCV Brown-Conrady
    /// distortion model. The intrinsic model has a single focal length (not fx
    /// and fy).
    OpenCV5,
    /// Tunes the 3D world points, the camera extrinsic parameters, and the
    /// camera intrinsic parameters including 4 distortion terms (2 radial
    /// distortions, 2 tangential distortions) in the OpenCV Brown-Conrady
    /// distortion model. The intrinsic model has a single focal length (not fx
    /// and fy).
    OpenCV4,
    /// Tunes the 3D world points, the camera extrinsic parameters, and the
    /// camera intrinsic parameters with no distortion terms. The intrinsic
    /// model has a single focal length (not fx and fy).
    OpenCV0,
    /// Tunes the 3D world points and the camera extrinsic parameters.  The
    /// intrinsic model can have a separate focal length for x and y directions.
    #[default]
    ExtrinsicsOnly,
}

pub(crate) struct CameraModelTypeInfo {
    pub(crate) num_distortion_params: usize,
    pub(crate) num_intrinsic_params: usize,
    pub(crate) num_extrinsic_params: usize,
    pub(crate) num_fixed_params: usize,
}

impl CameraModelTypeInfo {
    pub(crate) fn num_cam_params(&self) -> usize {
        self.num_intrinsic_params + self.num_extrinsic_params
    }
}

impl CameraModelType {
    pub(crate) fn info(&self) -> CameraModelTypeInfo {
        match self {
            CameraModelType::OpenCV5 => CameraModelTypeInfo {
                num_distortion_params: 5,
                num_intrinsic_params: 3 + 5,
                num_extrinsic_params: 6,
                num_fixed_params: 0,
            },
            CameraModelType::OpenCV4 => CameraModelTypeInfo {
                num_distortion_params: 4,
                num_intrinsic_params: 3 + 4,
                num_extrinsic_params: 6,
                num_fixed_params: 0,
            },
            CameraModelType::OpenCV0 => CameraModelTypeInfo {
                num_distortion_params: 0,
                num_intrinsic_params: 3,
                num_extrinsic_params: 6,
                num_fixed_params: 0,
            },
            CameraModelType::ExtrinsicsOnly => CameraModelTypeInfo {
                num_distortion_params: 0,
                num_intrinsic_params: 0,
                num_extrinsic_params: 6,
                num_fixed_params: 9, // fx, fy, cx, cy + 5 distortion
            },
        }
    }
}

impl CameraModelType {
    pub(crate) fn eval_cam_jacobians<F: na::RealField + Float>(
        &self,
        ba: &BundleAdjuster<F>,
        cam_num: NCamsType,
        pt_num: usize,
        j: &mut na::OMatrix<F, Dyn, Dyn>,
        cam_sub: ((usize, usize), (usize, usize)),
    ) {
        let pt = ba.points.column(pt_num);
        let [p_x, p_y, p_z] = [pt.x, pt.y, pt.z];

        let cam = &ba.cams[usize(cam_num)];
        let i = cam.intrinsics();
        let _cx = i.cx();
        let _cy = i.cy();
        let d = i.distortion.opencv_vec().as_slice();
        let [k1, k2, p1, p2, k3] = [d[0], d[1], d[2], d[3], d[4]];

        let e = cam.extrinsics();
        let rquat = e.pose().rotation;
        let abc = rquat.scaled_axis();
        let cc = e.camcenter();
        let [r_x, r_y, r_z] = [abc.x, abc.y, abc.z];
        let [w_x, w_y, w_z] = [cc.x, cc.y, cc.z];

        let num_cam_params = self.info().num_cam_params();

        // Jacobian for camera parameters.
        //
        // The symbolic expressions inside the per-variant `match` arms below
        // are produced by the SymPy scripts in `../codegen/`. A normal
        // `cargo build` does not run that codegen; see `../codegen/README.md`
        // for how (and when) to regenerate and re-embed the expressions.
        let (cam_start, cam_geom) = cam_sub;
        let mut j = j.view_mut(cam_start, cam_geom);
        debug_assert_eq!(j.nrows(), 2);
        debug_assert_eq!(j.ncols(), num_cam_params);

        match self {
            CameraModelType::OpenCV5 => {
                let f = i.fx(); // we checked in the constructor that fx == fy
                opencv5_cam_jacobian(
                    f,
                    k1,
                    k2,
                    p1,
                    p2,
                    k3,
                    r_x,
                    r_y,
                    r_z,
                    w_x,
                    w_y,
                    w_z,
                    p_x,
                    p_y,
                    p_z,
                    j.view_mut((0, 0), (2, 14)),
                );
            }
            CameraModelType::OpenCV4 => {
                let f = i.fx(); // we checked in the constructor that fx == fy
                opencv4_cam_jacobian(
                    f,
                    k1,
                    k2,
                    p1,
                    p2,        // no k3 in OpenCV4
                    F::zero(), // k3 = 0 (not used in OpenCV4)
                    r_x,
                    r_y,
                    r_z,
                    w_x,
                    w_y,
                    w_z,
                    p_x,
                    p_y,
                    p_z,
                    j.view_mut((0, 0), (2, 13)),
                );
            }

            CameraModelType::OpenCV0 => {
                let f = i.fx(); // we checked in the constructor that fx == fy
                opencv0_cam_jacobian(
                    f,
                    F::zero(), // k1 = 0
                    F::zero(), // k2 = 0
                    F::zero(), // p1 = 0
                    F::zero(), // p2 = 0
                    F::zero(), // k3 = 0
                    r_x,
                    r_y,
                    r_z,
                    w_x,
                    w_y,
                    w_z,
                    p_x,
                    p_y,
                    p_z,
                    j.view_mut((0, 0), (2, 9)),
                );
            }
            CameraModelType::ExtrinsicsOnly => {
                let fx = i.fx();
                let fy = i.fy();
                extrinsics_only_cam_jacobian(
                    fx,
                    fy,
                    k1,
                    k2,
                    p1,
                    p2,
                    k3,
                    r_x,
                    r_y,
                    r_z,
                    w_x,
                    w_y,
                    w_z,
                    p_x,
                    p_y,
                    p_z,
                    j.view_mut((0, 0), (2, 6)),
                );
            }
        }
    }
}
