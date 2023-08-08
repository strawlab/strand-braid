use anyhow::{Context, Result};

use flytrax_apriltags_calibration::{compute_extrinsics, ComputeExtrinsicsArgs};

const URL_BASE: &str = "https://strawlab-cdn.com/assets/flytrax-apriltags";

const IMAGE: &str = "flytrax20230621_133243.jpg";
const IMAGE_SUM: &str = "1ac0688ef39cc1c92b4aeeaf9bdd2416c6706aaf5def9cd6168a138e5b0dbf80";

const APRILCOORDS: &str = "apriltags_coordinates_1z.csv";
const APRILCOORDS_SUM: &str = "4d91b8449f5c69a0d5d77d307d479dbc30fac656f920ecd0dbf5ef2629d765d5";

const FLYTRAX: &str = "flytrax20230621_133243.csv";
const FLYTRAX_SUM: &str = "a27bd5639cf41d79b362e1f4acb1ddfd1924e05c62bfedc5d369ffc67a8add1f";

const INTRINSICS: &str = "Basler_22149788.20230621_162633.yaml";
const INTRINSICS_SUM: &str = "873222cdc71ac0a47d5d6dc0730b7d54fe68d6aac244cb7ffb04a154f58794d9";

#[test]
fn test_flytrax_apriltags() -> Result<()> {
    for (fname, sum) in [
        (IMAGE, IMAGE_SUM),
        (APRILCOORDS, APRILCOORDS_SUM),
        (FLYTRAX, FLYTRAX_SUM),
        (INTRINSICS, INTRINSICS_SUM),
    ] {
        download_verify::download_verify(
            format!("{}/{}", URL_BASE, fname).as_str(),
            fname,
            &download_verify::Hash::Sha256(sum.to_string()),
        )
        .with_context(|| format!("With file {fname} from {URL_BASE}/{fname}"))?;
    }

    let intrinsics_buf = std::fs::read_to_string(&INTRINSICS)?;

    let intrinsics: opencv_ros_camera::RosCameraInfo<f64> = serde_yaml::from_str(&intrinsics_buf)?;

    let args = ComputeExtrinsicsArgs {
        image_filename: IMAGE.into(),
        apriltags_3d_fiducial_coords: APRILCOORDS.into(),
        flytrax_csv: FLYTRAX.into(),
        intrinsics,
    };

    let results = compute_extrinsics(&args)?;

    let cam_names: Vec<_> = results.cal_result().mean_reproj_dist.keys().collect();
    assert_eq!(cam_names.len(), 1);
    let cam_name = cam_names[0];
    let reproj = results.cal_result().mean_reproj_dist[cam_name];
    assert!(reproj < 1.0);

    Ok(())
}
