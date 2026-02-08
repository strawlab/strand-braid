use camino::{Utf8Path, Utf8PathBuf};
use eyre::Result;
use std::io::{Read, Seek};
use strand_cam_offline_checkerboards::{Cli, run_cal};
use zip::ZipArchive;

const FNAME: &str = "checkerboard_debug_20240222_164128.zip";
const URL_BASE: &str = "http://strawlab-cdn.com/assets";
const SHA256SUM: &str = "a0bde56d50a33e580f9241d2b76674dc804ab10b2c1ffa60bb3bed43ac2ed9ed";

fn unpack_zip_into<R: Read + Seek>(
    mut archive: ZipArchive<R>,
    mcsc_dir_name: &Utf8Path,
) -> Result<()> {
    std::fs::create_dir_all(&mcsc_dir_name).unwrap();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        let outpath = match file.enclosed_name() {
            Some(path) => Utf8PathBuf::from_path_buf(path.to_owned()).unwrap(),
            None => continue,
        };
        let outpath = mcsc_dir_name.join(outpath);

        if (*file.name()).ends_with('/') {
            std::fs::create_dir_all(&outpath).unwrap();
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    std::fs::create_dir_all(p).unwrap();
                }
            }
            let mut outfile = std::fs::File::create(&outpath).unwrap();
            std::io::copy(&mut file, &mut outfile).unwrap();
        }
    }
    Ok(())
}

#[test]
fn test_checkerboard() -> Result<()> {
    let local_fname = format!("scratch/{FNAME}");
    download_verify::download_verify(
        format!("{}/{}", URL_BASE, FNAME).as_str(),
        &local_fname,
        &download_verify::Hash::Sha256(SHA256SUM.into()),
    )
    .unwrap();

    let data_root = tempfile::tempdir()?;
    let data_root_dir_name =
        Utf8PathBuf::from_path_buf(std::path::PathBuf::from(data_root.path())).unwrap();

    let rdr = std::fs::File::open(&local_fname)?;
    let cal_data_archive = ZipArchive::new(rdr)?;

    unpack_zip_into(cal_data_archive, &data_root_dir_name)?;

    let cli = Cli {
        input_dirname: data_root_dir_name.join("checkerboard_debug_20240222_164128"),
        pattern_width: 18,
        pattern_height: 8,
    };
    let cal = run_cal(cli)?;

    // Test results against those from a successful run. (Some deviation is
    // expected.)

    // camera_matrix is stored in row major order.
    approx::assert_relative_eq!(cal.camera_matrix[0], 1188.8, epsilon = 0.1); // fx
    approx::assert_relative_eq!(cal.camera_matrix[1], 0.0); // skew
    approx::assert_relative_eq!(cal.camera_matrix[2], 939.0, epsilon = 1.0); // cx
    approx::assert_relative_eq!(cal.camera_matrix[3], 0.0);
    approx::assert_relative_eq!(cal.camera_matrix[4], 1188.8, epsilon = 0.1); // fy
    approx::assert_relative_eq!(cal.camera_matrix[5], 583.0, epsilon = 1.0); // cy
    approx::assert_relative_eq!(cal.camera_matrix[6], 0.0);
    approx::assert_relative_eq!(cal.camera_matrix[7], 0.0);
    approx::assert_relative_eq!(cal.camera_matrix[8], 1.0);

    approx::assert_relative_eq!(cal.distortion_coeffs[0], -0.234, epsilon = 0.01);
    approx::assert_relative_eq!(cal.distortion_coeffs[1], 0.0754987651101312, epsilon = 0.01);
    approx::assert_relative_eq!(cal.distortion_coeffs[2], -7.954e-6, epsilon = 1e-5);
    approx::assert_relative_eq!(cal.distortion_coeffs[3], 6.39e-5, epsilon = 1e-5);

    Ok(())
}
