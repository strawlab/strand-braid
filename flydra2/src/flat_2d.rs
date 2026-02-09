use braid_mvg::DistortedPixel;
use flydra_mvg::MultiCamera;
use nalgebra::{Point3, RealField, Vector3};

pub(crate) fn ray_to_flat_3d(ray: &parry3d_f64::query::Ray) -> Option<Point3<f64>> {
    let z0 = parry3d_f64::shape::HalfSpace::new(Vector3::z_axis()); // build a plane from its center and normal, plane z==0 here.

    let solid = false; // will intersect either side of plane

    let opt_surface_pt_toi: Option<f64> =
        parry3d_f64::query::RayCast::cast_local_ray(&z0, ray, f64::max_value().unwrap(), solid);

    let ray_origin = ray.origin;
    let ray_dir = ray.dir;

    opt_surface_pt_toi.map(|toi| {
        let mut surface_pt = ray_origin + ray_dir * toi;
        // Due to numerical error, Z is not exactly zero. Here
        // we clamp it to zero.
        surface_pt.coords[2] = 0.0;
        surface_pt
    })
}

pub(crate) fn distorted_2d_to_flat_3d(
    cam: &MultiCamera<f64>,
    pt: &DistortedPixel<f64>,
) -> Option<Point3<f64>> {
    ray_to_flat_3d(&cam.project_distorted_pixel_to_ray(pt))
}
