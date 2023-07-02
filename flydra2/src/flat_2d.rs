use flydra_mvg::MultiCamera;
use mvg::DistortedPixel;
use nalgebra::{Point3, Vector3};
use ncollide3d::{query::Ray, shape::Plane};

use crate::MyFloat;

pub(crate) fn ray_to_flat_3d(ray: &Ray<f64>) -> Option<Point3<f64>> {
    let z0 = Plane::new(Vector3::z_axis()); // build a plane from its center and normal, plane z==0 here.
    let eye = nalgebra::Isometry3::identity();

    let solid = false; // will intersect either side of plane

    use ncollide3d::query::RayCast;
    let opt_surface_pt_toi: Option<MyFloat> = z0.toi_with_ray(&eye, ray, std::f64::MAX, solid);

    opt_surface_pt_toi.map(|toi| {
        let mut surface_pt = ray.origin + ray.dir * toi;
        // Due to numerical error, Z is not exactly zero. Here
        // we clamp it to zero.
        surface_pt.coords[2] = nalgebra::zero();
        surface_pt
    })
}

pub(crate) fn distorted_2d_to_flat_3d(
    cam: &MultiCamera<f64>,
    pt: &DistortedPixel<f64>,
) -> Option<Point3<f64>> {
    ray_to_flat_3d(&cam.project_distorted_pixel_to_ray(pt))
}
