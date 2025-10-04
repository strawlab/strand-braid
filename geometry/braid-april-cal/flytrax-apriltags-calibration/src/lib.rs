use std::path::Path;

use braid_april_cal::*;
use eyre::{self as anyhow, Context};
use flytrax_io::read_csv_commented_header;
use machine_vision_formats::pixel_format::Mono8;
use opencv_ros_camera::NamedIntrinsicParameters;

use ads_apriltag as apriltag;
use ads_webasm::components::{parse_csv, MaybeCsvData};

use apriltag_detection_writer::AprilConfig;

mod img_write;
mod tiny_skia_frame;

struct AprilTagCoords2D {
    id: i32,
    hamming: i32,
    x: f64,
    y: f64,
}

fn read_apriltags<P: AsRef<std::path::Path>>(
    fname: P,
) -> anyhow::Result<(Vec<AprilTagCoords2D>, Vec<u8>)> {
    let mut td = apriltag::Detector::new();
    let tf = apriltag::Family::new_tag_36h11();
    td.add_family(tf);

    let raw_td = td.as_mut();
    // raw_td.debug = 1;
    raw_td.quad_decimate = 2.0;
    raw_td.quad_sigma = 0.0;
    raw_td.refine_edges = 1;
    raw_td.decode_sharpening = 0.25;

    let jpeg_buf =
        std::fs::read(&fname).with_context(|| format!("reading {}", fname.as_ref().display()))?;
    let image = image::load_from_memory_with_format(&jpeg_buf, image::ImageFormat::Jpeg)
        .with_context(|| format!("parsing {}", fname.as_ref().display()))?;

    let rgb = convert_image::image_to_rgb8(image)?;

    let dest = convert_image::convert_ref::<_, Mono8>(&rgb)?;
    let im = apriltag::ImageU8Borrowed::view(&dest);
    let detections = td.detect(apriltag::ImageU8::inner(&im));

    tracing::info!(
        "In image file {}, got {} detection(s).",
        fname.as_ref().display(),
        detections.len()
    );

    let res = detections
        .as_slice()
        .iter()
        .map(|det| {
            // {
            //     println!("  {{id: {}, center: {:?}}}", det.id(), det.center(),);
            // }

            let c = det.center();
            AprilTagCoords2D {
                id: det.id(),
                hamming: det.hamming(),
                x: c[0],
                y: c[1],
            }
        })
        .collect();

    Ok((res, jpeg_buf))
}

#[derive(Debug, Clone)]
pub struct AprilTagReprojectedPoint<R: nalgebra::RealField> {
    pub id: i32,
    pub projected_point: [R; 2],
    pub detected_point: [R; 2],
}

pub struct SingleCamCalResults {
    cal_result: CalibrationResult,
    reproj: Vec<AprilTagReprojectedPoint<f64>>,
    jpeg_buf: Vec<u8>,
    named_intrinsics: NamedIntrinsicParameters<f64>,
}

impl SingleCamCalResults {
    pub fn cal_result(&self) -> &CalibrationResult {
        &self.cal_result
    }
}

pub struct ComputeExtrinsicsArgs {
    /// CSV file with April Tags 3D fiducial coordinates.
    pub apriltags_3d_fiducial_coords: camino::Utf8PathBuf,

    /// camera intrinsics.
    pub intrinsics: opencv_ros_camera::RosCameraInfo<f64>,

    /// JPEG image with april tags which will be detected.
    ///
    /// This is typically the JPEG saved alongside
    /// the flytrax CSV file.
    pub image_filename: camino::Utf8PathBuf,

    /// CSV data from the experiment.
    pub flytrax_csv: camino::Utf8PathBuf,
}

pub fn compute_extrinsics(cli: &ComputeExtrinsicsArgs) -> anyhow::Result<SingleCamCalResults> {
    // read all files for calibration -----
    // April Tag 3D coordinates file
    let fiducial_coords_buf =
        std::fs::read(&cli.apriltags_3d_fiducial_coords).with_context(|| {
            format!(
                "when reading April Tag 3D coordinates CSV file \"{}\"",
                cli.apriltags_3d_fiducial_coords
            )
        })?;
    let fiducial_coords = parse_csv::<Fiducial3DCoords>(
        format!("{}", cli.apriltags_3d_fiducial_coords),
        &fiducial_coords_buf,
    );
    let fiducial_3d_coords = match fiducial_coords {
        MaybeCsvData::Valid(data) => data.rows().to_vec(),
        MaybeCsvData::ParseFail(e) => {
            anyhow::bail!(
                "failed parsing file {}: {e}",
                cli.apriltags_3d_fiducial_coords
            );
        }
        MaybeCsvData::Empty => {
            anyhow::bail!("empty file {}", cli.apriltags_3d_fiducial_coords);
        }
    };

    tracing::info!(
        "In fiducial coordinates file {}, got {} fiducial marker(s).",
        cli.apriltags_3d_fiducial_coords,
        fiducial_3d_coords.len()
    );

    // for f3c in fiducial_3d_coords.iter() {
    //     println!(
    //         "  {{id: {}: x: {}, y: {}, z: {}}}",
    //         f3c.id, f3c.x, f3c.y, f3c.z
    //     );
    // }

    let flytrax_header = {
        let point_detection_csv_reader = std::fs::File::open(&cli.flytrax_csv)
            .with_context(|| format!("opening {}", cli.flytrax_csv))?;
        let mut point_detection_csv_reader = std::io::BufReader::new(point_detection_csv_reader);

        read_csv_commented_header(&mut point_detection_csv_reader)
            .with_context(|| format!("parsing header from {}", cli.flytrax_csv))?
    };

    let camera_name = flytrax_header.camera.model;

    let named_intrinsics = {
        let mut named_intrinsics: NamedIntrinsicParameters<f64> =
            cli.intrinsics.clone().try_into().unwrap();
        let orig_name = named_intrinsics.name.clone();
        if named_intrinsics.name != camera_name {
            // Ensure calibration is really for this camera.
            let sub_name = camera_name.replace('-', "_");
            if sub_name != orig_name {
                anyhow::bail!(
                    "Camera name unknown? In intrinsics YAML file, it is {orig_name}. \
                In flytrax CSV file, it is {camera_name} (which might get changed to {sub_name})."
                );
            }
            // Would like to use name in .yaml file, but this has been converted to
            // "ROS form". Therefore, we get it from the flytrax .csv file.
            named_intrinsics.name = camera_name.clone();
        }
        named_intrinsics
    };

    // Convert to needed format for calibration
    let known_good_intrinsics = {
        let mut known_good_intrinsics = std::collections::BTreeMap::new();
        known_good_intrinsics.insert(named_intrinsics.name.clone(), named_intrinsics.clone());
        Some(known_good_intrinsics)
    };

    // Extract April tags locations from image file by doing detections.
    let (per_camera_2d, jpeg_buf, detections) = {
        let (detections, jpeg_buf) = read_apriltags(&cli.image_filename)?;

        let mut per_camera_2d = std::collections::BTreeMap::new();

        let detections2: Vec<_> = detections
            .iter()
            .map(|d| AprilDetection {
                frame: 0,
                id: d.id,
                hamming: d.hamming,
                h02: d.x,
                h12: d.y,
            })
            .collect();

        per_camera_2d.insert(
            camera_name.clone(),
            (
                AprilConfig {
                    created_at: flytrax_header.created_at,
                    camera_height_pixels: flytrax_header.camera.height.try_into().unwrap(),
                    camera_width_pixels: flytrax_header.camera.width.try_into().unwrap(),
                    camera_name: camera_name.clone(),
                },
                detections2,
            ),
        );
        (per_camera_2d, jpeg_buf, detections)
    };

    let src_data = CalData {
        fiducial_3d_coords,
        per_camera_2d,
        known_good_intrinsics,
    };

    let cal_result = braid_april_cal::run_sqpnp_or_dlt(&src_data)?;

    tracing::info!(
        "Calibration result for {}: {:.2} pixel mean reprojection distance",
        camera_name,
        cal_result.mean_reproj_dist[&camera_name]
    );

    let reproj = {
        let mut reproj = Vec::new();
        let points = cal_result.points.get(&camera_name).unwrap();
        let cam = cal_result.cam_system.cam_by_name(&camera_name).unwrap();

        for detect in detections.iter() {
            let mut found = None;
            for test_pt in points.iter() {
                if test_pt.id == detect.id {
                    found = Some(test_pt);
                    break;
                }
            }

            if let Some(found) = found {
                let world_pt = braid_mvg::PointWorldFrame {
                    coords: nalgebra::Point3::from_slice(&found.object_point),
                };
                let projected_pixel = cam.project_3d_to_distorted_pixel(&world_pt);

                reproj.push(AprilTagReprojectedPoint {
                    id: detect.id,
                    detected_point: [detect.x, detect.y],
                    projected_point: [projected_pixel.coords.x, projected_pixel.coords.y],
                });
            }
        }
        reproj
    };

    Ok(SingleCamCalResults {
        cal_result,
        reproj,
        jpeg_buf,
        named_intrinsics,
    })
}

pub fn save_cal_result_to_xml<P: AsRef<Path>>(
    output_xml: P,
    res: &SingleCamCalResults,
) -> anyhow::Result<()> {
    let SingleCamCalResults {
        cal_result,
        reproj: _,
        jpeg_buf: _,
        named_intrinsics: _,
    } = res;
    let xml_buf = cal_result.to_flydra_xml()?;
    std::fs::write(output_xml.as_ref(), xml_buf)?;
    tracing::info!("Saved output XML to: {}", output_xml.as_ref().display());

    Ok(())
}

pub fn save_cal_svg_and_png_images<P: AsRef<Path>>(
    out_svg_fname: P,
    res: &SingleCamCalResults,
) -> anyhow::Result<()> {
    let SingleCamCalResults {
        cal_result: _,
        reproj,
        jpeg_buf,
        named_intrinsics,
    } = res;
    let jpeg_buf = jpeg_buf.clone();
    let reproj = reproj.clone();

    let pcr = img_write::PerCamRender {
        width: named_intrinsics.width,
        height: named_intrinsics.height,
    };
    let pcrf = img_write::PerCamRenderFrame {
        p: &pcr,
        jpeg_buf,
        reproj,
    };

    img_write::doit(&out_svg_fname, &pcrf)?;
    tracing::info!(
        "Saved image for debugging to: {}",
        out_svg_fname.as_ref().display()
    );

    Ok(())
}
