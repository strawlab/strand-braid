use flydra_mvg::FlydraMultiCameraSystem;
use mvg::PointWorldFrame;
use nalgebra::geometry::Point3;

use std::convert::TryInto;

type MyType = f64;

pub const CALIBRATION_FILE: &str = include_str!("aligned-water.xml");

pub fn do_test(cams: &FlydraMultiCameraSystem<MyType>, data: &[u8], verbose: bool) {
    if data.len() != 24 {
        // panic!("wrong size");
        return;
    }
    let x = MyType::from_be_bytes(data[0..8].try_into().unwrap());
    let y = MyType::from_be_bytes(data[8..16].try_into().unwrap());
    let z = MyType::from_be_bytes(data[16..24].try_into().unwrap());

    let pt3d = PointWorldFrame {
        coords: Point3::new(x, y, z),
    };

    for el in pt3d.coords.iter() {
        if !el.is_finite() {
            return;
        }
    }

    if verbose {
        println!("pt3d {:?}", pt3d);
    }
    for cam in cams.cameras() {
        if verbose {
            println!("cam {}", cam.name());
        }
        let result = cam.project_3d_to_ray(&pt3d);
        if verbose {
            println!("result {:?}", result);
        }
    }
}
