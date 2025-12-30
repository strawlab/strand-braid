use cam_geom::ExtrinsicParameters;
use na::{Matrix3xX, Vector3};
use nalgebra as na;

use braid_mvg::rerun_io::AsRerunTransform3D;

fn main() -> eyre::Result<()> {
    // Create 3d points.
    #[rustfmt::skip]
    let points3d = Matrix3xX::<f64>::from_column_slice(
        &[
        0.0, 0.0, 0.0,
        1.0, 0.0, 0.0,
        1.0, 1.0, 0.0,
        0.0, 1.0, 0.0,
        0.0, 0.0, 1.0,
        1.0, 0.0, 1.0,
        1.0, 1.0, 1.0,
        0.0, 1.0, 1.0,
    ]);

    // Create rerun file
    let rec =
        re_sdk::RecordingStreamBuilder::new("export-rerun-log").save("export-rerun-log.rrd")?;

    // Log 3d points to rerun.
    let positions: Vec<[f32; 3]> = points3d
        .column_iter()
        .map(|v| [v.x as f32, v.y as f32, v.z as f32])
        .collect();
    rec.log("/", &re_sdk_types::archetypes::Points3D::new(&positions))?;

    // Create camera
    let cc = Vector3::new(3.0, 2.0, 1.0);
    let lookat = Vector3::new(0.0, 0.0, 0.0);
    let up = Vector3::new(0.0, 0.0, 1.0);
    let up_unit = na::core::Unit::new_normalize(up);

    let extrinsics = ExtrinsicParameters::from_view(&cc, &lookat, &up_unit);

    let width = 640;
    let height = 480;

    let params = cam_geom::PerspectiveParams {
        fx: 100.0,
        fy: 101.0,
        skew: 0.0,
        cx: width as f64 / 2.0,
        cy: height as f64 / 2.0,
    };
    let intrinsics: cam_geom::IntrinsicParametersPerspective<_> = params.into();

    let cam = cam_geom::Camera::new(intrinsics, extrinsics);

    // Log camera extrinsics to rerun.
    rec.log(
        "/world/camera/cam1",
        &cam.extrinsics().as_rerun_transform3d().into(),
    )
    .unwrap();

    // Log camera intrinsics to rerun.
    rec.log(
        "/world/camera/cam1/raw",
        &braid_mvg::rerun_io::cam_geom_to_rr_pinhole_archetype(cam.intrinsics(), width, height)?,
    )
    .unwrap();

    // Project camera to points
    let world = cam_geom::Points::new(points3d.transpose());
    let pixels = cam.world_to_pixel(&world);

    // Log 2D points to rerun
    let points2d: Vec<[f32; 2]> = pixels
        .data
        .row_iter()
        .map(|v| {
            let v = v.transpose();
            [v.x as f32, v.y as f32]
        })
        .collect();
    rec.log(
        "/world/camera/cam1/raw",
        &re_sdk_types::archetypes::Points2D::new(&points2d),
    )?;

    Ok(())
}
