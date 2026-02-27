use anyhow::Context;
use serde::{Deserialize, Serialize};
use tracing::info;

use nalgebra::geometry::{Point2, Point3};

mod error;
mod exr;
pub mod pinhole_wizard_yaml_support;
mod trimesh_ext;
use trimesh_ext::FaceIndices;

use std::path::Path;

pub use crate::exr::ExrWriter;
pub use crate::pinhole_wizard_yaml_support::{
    compute_mask, merge_vdisps, parse_obj_from_reader, solve_no_distortion_display_camera,
    FromFileGeom, Geom, LoadedPinholeInputFile, MultiDisplayInputFile, PinholeInputFile,
    SimplePinholeInputFile, SphereGeom, TriMeshGeom,
};
pub use error::Error;
pub mod types;
pub use types::VDispInfo;

use ncollide_geom::Mask;

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
/// Used to compute intrinsic camera parameters
pub struct Checkerboard {
    #[serde(rename = "size", skip_serializing, default)]
    _size: f64,
    #[serde(rename = "columns")]
    n_cols: usize,
    #[serde(rename = "rows")]
    n_rows: usize,
    date_string: String,
    #[serde(rename = "points")]
    corners: Vec<(f64, f64)>,
}

pub struct FloatImage {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) pixels: std::vec::Vec<(f64, f64, f64)>,
}

impl FloatImage {
    pub fn from_data(width: usize, height: usize, rgb_data: Vec<f64>) -> Self {
        assert_eq!(rgb_data.len(), width * height * 3);
        let pixels = rgb_data
            .chunks(3)
            .map(|rgb| (rgb[0], rgb[1], rgb[2]))
            .collect();
        Self {
            width,
            height,
            pixels,
        }
    }
    pub fn sample(&self, row: usize, col: usize) -> (f64, f64, f64) {
        let idx = row * (self.width) + col;
        self.pixels[idx]
    }
}

#[cfg(feature = "opencv")]
fn to_camcal(board: &Checkerboard) -> camcal::CheckerBoardData {
    let corners: Vec<(f64, f64)> = board.corners.clone();
    camcal::CheckerBoardData::new(board.n_rows, board.n_cols, &corners)
}

pub fn as_ncollide_mesh(tm: &textured_tri_mesh::TriMesh) -> ncollide3d::shape::TriMesh<f64> {
    fn usize(x: u32) -> usize {
        x.try_into().unwrap()
    }
    let coords = tm
        .coords
        .iter()
        .map(|x| Point3::new(x[0], x[1], x[2]))
        .collect();
    let indices = tm
        .indices
        .iter()
        .map(|x| Point3::new(usize(x[0]), usize(x[1]), usize(x[2])))
        .collect();
    let uvs = Some(tm.uvs.iter().map(|x| Point2::new(x[0], x[1])).collect());
    ncollide3d::shape::TriMesh::new(coords, indices, uvs)
}

fn parse_multi_display_yaml<R: std::io::Read>(
    mut reader: R,
) -> Result<pinhole_wizard_yaml_support::MultiDisplayInputFile> {
    let mut buf = String::new();
    reader.read_to_string(&mut buf)?;
    let result: pinhole_wizard_yaml_support::MultiDisplayInputFile =
        match serde_yaml::from_str(&buf) {
            Ok(result) => result,
            Err(e1) => {
                return Err(Error::FailedParse1(e1));
            }
        };
    Ok(result)
}

pub fn parse_pinhole_yaml<R: std::io::Read, P: AsRef<Path>>(
    mut reader: R,
    yaml_dir: P,
) -> Result<pinhole_wizard_yaml_support::LoadedPinholeInputFile> {
    let mut buf = String::new();
    reader.read_to_string(&mut buf)?;
    let result: pinhole_wizard_yaml_support::PinholeInputFile = match serde_yaml::from_str(&buf) {
        Ok(result) => result,
        Err(err1) => {
            match serde_yaml::from_str::<crate::pinhole_wizard_yaml_support::SimplePinholeInputFile>(
                &buf,
            ) {
                Ok(simple) => simple.to_orig(),
                Err(err2) => {
                    return Err(Error::FailedParse { err1, err2 });
                }
            }
        }
    };
    let result = LoadedPinholeInputFile {
        loaded: result,
        _yaml_dir: yaml_dir.as_ref().to_path_buf(),
    };
    Ok(result)
}

#[cfg(feature = "opencv")]
pub fn intrinsics_from_checkerboards(
    checkerboards: &[Checkerboard],
    width: usize,
    height: usize,
) -> Result<opencv_ros_camera::RosOpenCvIntrinsics<f64>> {
    let size = camcal::PixelSize::new(width, height);
    let goodcorners: Vec<camcal::CheckerBoardData> = checkerboards.iter().map(to_camcal).collect();
    Ok(camcal::compute_intrinsics(size, &goodcorners)?)
}

fn blit_data(src: &FloatImage, dest: &mut FloatImage, x: usize, y: usize) -> Result<()> {
    for src_row in 0..src.height {
        let dest_row = src_row + y;
        for src_col in 0..src.width {
            let dest_col = src_col + x;
            let src_idx = src_row * src.width + src_col;
            let dest_idx = dest_row * dest.width + dest_col;
            dest.pixels[dest_idx] = src.pixels[src_idx];
        }
    }
    Ok(())
}

pub fn do_multi_display<R: std::io::Read, P: AsRef<Path>>(
    fd: R,
    epsilon: f64,
    src_dir: P,
) -> Result<FloatImage> {
    let data = parse_multi_display_yaml(fd)?;
    println!("loaded file {:?}", data);

    let rgb_data: Vec<f64> = vec![-1.0; data.final_size.width * data.final_size.height * 3];
    let mut full_image =
        FloatImage::from_data(data.final_size.width, data.final_size.height, rgb_data);

    for display in data.displays.iter() {
        let orig_path = &display.calibration_file;
        let cal_file = if orig_path.is_absolute() {
            orig_path.clone()
        } else {
            let mut new_base = src_dir.as_ref().to_path_buf();
            new_base.push(orig_path);
            new_base
        };

        let fd = std::fs::File::open(&cal_file)
            .context(format!("opening file: {}", cal_file.display()))?;
        let src_data = ActualFiles::new(fd, &src_dir, epsilon)?;
        let this_float_image = fit_pinholes_compute_cal_image(&src_data, false, false)?;
        blit_data(&this_float_image, &mut full_image, display.x, display.y)?;
    }
    Ok(full_image)
}

// Implements `PinholeCal` trait in a way that does not require disk access.
// TODO: Note, the `PinholeCal` trait (and `PinholeCalData` struct) should be
// removed now that `PinholeCalib` trait exists.
#[derive(Debug, Serialize, Deserialize)]
pub struct PinholeCalData {
    data: crate::pinhole_wizard_yaml_support::SimplePinholeNoFile,
    // display: crate::types::SimpleDisplay,
    geom: TriMeshGeom,
    pinhole_fits: Vec<(types::VirtualDisplayName, braid_mvg::Camera<f64>)>,
}

impl PinholeCalData {
    pub fn new(
        display: crate::types::SimpleDisplay,
        geom: TriMeshGeom,
        uv_display_points: Vec<crate::types::SimpleUVCorrespondance>,
        epsilon: f64,
    ) -> Result<Self> {
        let data = crate::pinhole_wizard_yaml_support::SimplePinholeNoFile {
            display,
            uv_display_points,
        };
        let pinhole_fits = solve_no_distortion_display_camera(&data, &geom, epsilon)?;

        Ok(Self {
            data,
            pinhole_fits,
            // display,
            geom,
        })
    }
}

// Implements `PinholeCal` trait in a way that requires disk access.
// TODO: Note, the `PinholeCal` trait (and `ActualFiles` struct) should be
// removed now that `PinholeCalib` trait exists.
pub struct ActualFiles {
    data: pinhole_wizard_yaml_support::LoadedPinholeInputFile,
    loaded_geom: Box<dyn DisplayGeometry>,
    trimesh: Option<TriMeshGeom>,
    pinhole_fits: Vec<(types::VirtualDisplayName, braid_mvg::Camera<f64>)>,
}

impl ActualFiles {
    pub fn new<R: std::io::Read, P: AsRef<Path>>(fd: R, src_dir: P, epsilon: f64) -> Result<Self> {
        let yaml_dir = src_dir.as_ref().to_path_buf();
        let data = parse_pinhole_yaml(fd, &yaml_dir)?;

        let trimesh = data.loaded.geom.as_trimesh(&yaml_dir).ok();

        let loaded_geom = data.loaded.geom.load_geom(&yaml_dir)?;
        info!("parsed input file");

        let pinhole_fits =
            solve_no_distortion_display_camera(&data.loaded, loaded_geom.as_ref(), epsilon)?;

        Ok(Self {
            data,
            loaded_geom,
            trimesh,
            pinhole_fits,
        })
    }
}

// TODO remove this trait
pub trait PinholeCal {
    fn pinhole_fits(&self) -> &[(types::VirtualDisplayName, braid_mvg::Camera<f64>)];
    fn display_width_height(&self) -> (usize, usize);
    fn vdisp_mask(
        &self,
        name: &types::VirtualDisplayName,
    ) -> Result<ncollide2d::shape::Compound<f64>>;
    fn geom(&self) -> &dyn DisplayGeometry;
    fn geom_as_trimesh(&self) -> Option<&TriMeshGeom>;
    fn merge_virtual_displays(&self, vdisp_data: &[&VDispInfo], show_mask: bool) -> Vec<f64>;
}

impl PinholeCal for ActualFiles {
    fn pinhole_fits(&self) -> &[(types::VirtualDisplayName, braid_mvg::Camera<f64>)] {
        &self.pinhole_fits
    }
    fn display_width_height(&self) -> (usize, usize) {
        use crate::pinhole_wizard_yaml_support::PinholeCalib;
        (self.data.loaded.width(), self.data.loaded.height())
    }
    fn vdisp_mask(
        &self,
        name: &types::VirtualDisplayName,
    ) -> Result<ncollide2d::shape::Compound<f64>> {
        compute_mask(&self.data.loaded, name)
    }
    fn geom(&self) -> &dyn DisplayGeometry {
        self.loaded_geom.as_ref()
    }
    fn geom_as_trimesh(&self) -> Option<&TriMeshGeom> {
        self.trimesh.as_ref()
    }
    fn merge_virtual_displays(&self, vdisp_data: &[&VDispInfo], show_mask: bool) -> Vec<f64> {
        merge_vdisps(&self.data.loaded, vdisp_data, show_mask)
    }
}

impl PinholeCal for PinholeCalData {
    fn pinhole_fits(&self) -> &[(types::VirtualDisplayName, braid_mvg::Camera<f64>)] {
        &self.pinhole_fits
    }
    fn display_width_height(&self) -> (usize, usize) {
        use crate::pinhole_wizard_yaml_support::PinholeCalib;
        (self.data.width(), self.data.height())
    }
    fn vdisp_mask(
        &self,
        name: &types::VirtualDisplayName,
    ) -> Result<ncollide2d::shape::Compound<f64>> {
        compute_mask(&self.data, name)
    }
    fn geom(&self) -> &dyn DisplayGeometry {
        &self.geom
    }
    fn geom_as_trimesh(&self) -> Option<&TriMeshGeom> {
        Some(&self.geom)
    }
    fn merge_virtual_displays(&self, vdisp_data: &[&VDispInfo], show_mask: bool) -> Vec<f64> {
        merge_vdisps(&self.data, vdisp_data, show_mask)
    }
}

pub fn fit_pinholes_compute_cal_image(
    src_cal: &dyn PinholeCal,
    save_debug_images: bool,
    show_mask: bool,
) -> Result<FloatImage> {
    let vdisp_data = compute_vdisp_images(src_cal, save_debug_images, show_mask)?;
    let view: Vec<&VDispInfo> = vdisp_data.iter().collect();
    merge_vdisp_images(&view, src_cal, save_debug_images, show_mask)
}

pub fn compute_vdisp_images(
    src_cal: &dyn PinholeCal,
    save_debug_images: bool,
    show_mask: bool,
) -> Result<Vec<VDispInfo>> {
    let pinhole_fits = src_cal.pinhole_fits();
    let mut vdisp_data = Vec::new();
    let (width, height) = src_cal.display_width_height();
    for (i, (name, cam)) in pinhole_fits.iter().enumerate() {
        info!("computed camera for virtual display {}: {:?}", i, name);
        let mask = src_cal.vdisp_mask(name)?;
        let (texcoords, nchan) =
            compute_image_for_camera_view(cam, Computable::TexCoords, src_cal.geom(), &mask)?;

        if save_debug_images {
            let fname = format!("vdisp_{}.jpg", i);
            let mask_arg = if show_mask { Some(&mask) } else { None };
            debug_image(
                &texcoords,
                height as u32,
                width as u32,
                nchan,
                &fname,
                mask_arg,
            )
            .unwrap();
        }
        vdisp_data.push((mask, texcoords, nchan));
    }
    Ok(vdisp_data)
}

pub fn merge_vdisp_images(
    vdisp_data: &[&VDispInfo],
    src_cal: &dyn PinholeCal,
    save_debug_images: bool,
    show_mask: bool,
) -> Result<FloatImage> {
    let cal_data = src_cal.merge_virtual_displays(vdisp_data, show_mask);

    let (width, height) = src_cal.display_width_height();

    if save_debug_images {
        debug_image(&cal_data, height as u32, width as u32, 3, "out.jpg", None).unwrap();
    }

    let float_image = FloatImage::from_data(width, height, cal_data);
    Ok(float_image)
}

/// Save a debug image to new file with name `fname`.
fn debug_image(
    buf: &[f64],
    height: u32,
    width: u32,
    nchan: usize,
    fname: &str,
    mask: Option<&Mask>,
) -> Result<()> {
    let quality = 99;
    let mut rgb: Vec<u8> = match nchan {
        3 => buf.iter().map(|el| (el * 255.0).trunc() as u8).collect(),
        2 => {
            buf.chunks(2)
                .flat_map(|uv| {
                    let u = uv[0];
                    let v = uv[1];

                    if u.is_nan() {
                        // black if no texcoord
                        vec![0, 0, 0]
                    } else {
                        // red = U scaled
                        // green = V scaled
                        // blue = 0
                        vec![(u * 255.0).trunc() as u8, (v * 255.0).trunc() as u8, 255u8]
                    }
                })
                .collect()
        }
        nchan => {
            unimplemented!("support for nchan {}", nchan);
        }
    };
    let m = nalgebra::geometry::Isometry::identity();
    if let Some(mask) = mask {
        for row in 0..height as usize {
            for col in 0..width as usize {
                let start = (row * width as usize + col) * 3;
                let cur_pos = nalgebra::geometry::Point2::new(col as f64, row as f64);
                use ncollide2d::query::point_query::PointQuery;
                if mask.distance_to_point(&m, &cur_pos, true) < 1.0 {
                    // blue channel = is in mask?
                    rgb[start + 2] = 255;
                }
            }
        }
    }
    let mut jpeg_buf = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buf, quality);
    encoder.encode(&rgb, width, height, image::ColorType::Rgb8.into())?;
    let mut f = std::fs::File::create(fname)?;
    {
        use std::io::Write;
        f.write_all(&jpeg_buf)?;
    }
    Ok(())
}

pub trait DisplayGeometry {
    /// Compute world coordinate for a given texture coordinate
    fn texcoord2worldcoord(&self, tc: &Point2<f64>) -> Option<Point3<f64>>;
    /// Compute texture coordinate for a given world coordinate
    fn worldcoord2texcoord(&self, surface_pt: &Point3<f64>) -> Option<Point2<f64>>;
    /// Return a trait object allowing ray casting with the display
    fn ncollide_shape(&self) -> &dyn ncollide3d::query::RayCast<f64>;
    /// Intersect a ray with the display and compute something (e.g. texcoords)
    fn intersect(&self, ray: &ncollide3d::query::Ray<f64>, compute: Computable) -> Computed {
        let solid = true; // TODO: check if we should set to false to intersect either side of shape.
        let eye = nalgebra::Isometry3::identity();

        // let opt_toi: Option<f64> = self.ncollide_shape().toi_with_ray(&eye, ray, solid);

        let opt_ray_intersect =
            self.ncollide_shape()
                .toi_and_normal_and_uv_with_ray(&eye, ray, f64::MAX, solid);

        match compute {
            Computable::TexCoords => {
                match opt_ray_intersect {
                    Some(ray_intersect) => {
                        // let surface_pt = ray.origin + ray.dir * toi;
                        // match self.worldcoord2texcoord(&surface_pt) {
                        //     Some(tc) => Computed::TexCoords((tc[0],tc[1])),
                        //     None => {
                        //         panic!("no intersection but TOI: {:?}", surface_pt);
                        //         // Computed::TexCoords((f64::NAN,f64::NAN))
                        //     },
                        // }
                        let tc = ray_intersect.uvs.unwrap(); // we know we have uvs for our shapes, so unwrap() is ok
                        Computed::TexCoords((tc[0], tc[1]))
                    }
                    None => Computed::TexCoords((f64::NAN, f64::NAN)),
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Computable {
    TexCoords,
}

#[derive(Debug, Clone)]
pub enum Computed {
    /// texture coordinates (can be nan)
    TexCoords((f64, f64)),
}

/// Given a camera and a geometry, compute something (e.g. texture coordinates).
pub fn compute_image_for_camera_view(
    cam: &braid_mvg::Camera<f64>,
    show: Computable,
    geom: &dyn DisplayGeometry,
    mask: &Mask,
) -> Result<(Vec<f64>, usize)> {
    let center = cam.extrinsics().camcenter();

    let nchan = match show {
        Computable::TexCoords => 2, // U, V
    };

    let mut result = vec![f64::NAN; cam.width() * cam.height() * nchan];

    // println!("-------- CAMERA");
    // println!("pmat {}", pretty_print_nalgebra::pretty_print!(cam.as_pmat().unwrap()));
    // println!("cc {}", pretty_print_nalgebra::pretty_print!(cam.extrinsics().camcenter().coords));
    // println!("forward {}", pretty_print_nalgebra::pretty_print!(cam.extrinsics().forward()));
    // println!("up {}", pretty_print_nalgebra::pretty_print!(cam.extrinsics().up()));

    info!("computing image for camera view");

    let m = nalgebra::geometry::Isometry::identity();
    for camy in 0..cam.height() {
        for camx in 0..cam.width() {
            let coords = nalgebra::geometry::Point2::new(camx as f64, camy as f64);
            use ncollide2d::query::point_query::PointQuery;
            if mask.distance_to_point(&m, &coords, true) > 1.0 {
                // not in masked region, skip this pixel
                continue;
            }

            let cam_px = braid_mvg::DistortedPixel { coords };

            // let undist_px = cam.intrinsics().undistort(&cam_px);
            // let pt_cam = cam.intrinsics().project_pixel_to_3d_camera_with_dist(&undist_px, 1.0);
            let world_coord = cam.project_distorted_pixel_to_3d_with_dist(&cam_px, 1.0);

            let dir = world_coord.coords - center;
            debug_assert!((dir.magnitude_squared() - 1.0).abs() < 1e-10); // ensure unit distance
            let ray = ncollide3d::query::Ray::new(*center, dir);

            let start = (camy * cam.width() + camx) * nchan;
            // let stop = start+nchan;
            let tc = geom.intersect(&ray, show);
            match tc {
                Computed::TexCoords(uv) => {
                    result[start] = uv.0;
                    result[start + 1] = uv.1;
                }
            }

            // if camx <2 && camy < 2 {
            //     println!("--------");
            //     println!("cam_px = {:?}", cam_px);
            //     println!("pt_cam at dist 1.0 = {}", pretty_print_nalgebra::pretty_print!(pt_cam.coords.coords));
            //     println!("world_coord at dist 1.0 = {}", pretty_print_nalgebra::pretty_print!(world_coord.coords.coords));
            //     println!("dir = {}", pretty_print_nalgebra::pretty_print!(dir));
            //     println!("surface result = {:?}", &result[start..stop]);
            // }
        }
    }

    Ok((result, nchan))
}

pub fn csv2exr<R, W>(
    corr_points_csv: R,
    out_wtr: &mut W,
    save_debug_images: bool,
    exr_comment: Option<&str>,
) -> Result<()>
where
    R: std::io::Read + std::io::Seek,
    W: std::io::Write,
{
    // Step 1 - read the CSV header for the display width and height
    use std::io::BufRead;
    use std::io::Seek;

    let mut buf_reader = std::io::BufReader::new(corr_points_csv);
    let mut width = None;
    let mut height = None;
    loop {
        let mut line = String::new();
        buf_reader.read_line(&mut line)?;
        if line.starts_with("#") {
            const PARAMS: &str = "# params: ";
            if let Some(comment_params_buf) = line.strip_prefix(PARAMS) {
                let comment_params: CommentParams = serde_json::from_str(comment_params_buf)?;
                width = Some(comment_params.display_width);
                height = Some(comment_params.display_height);
            }
        } else {
            // done with initial comments
            break;
        }
    }
    buf_reader.seek(std::io::SeekFrom::Start(0))?;

    let width = width.ok_or(Error::DisplaySizeNotFound)?;
    let height = height.ok_or(Error::DisplaySizeNotFound)?;

    // Step 2 - read the CSV file into 3 `Vec<Point3>`: texcoords, world coords, and display.
    let mut rdr = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .from_reader(buf_reader);
    let mut csv_texcoords_3d = Vec::new();
    let mut csv_texcoords_2d = Vec::new();
    let mut csv_worldcoords = Vec::new();
    let mut csv_displaycoords = Vec::new();
    for (row_num, result) in rdr.deserialize().enumerate() {
        let row: crate::types::CompleteCorrespondance = result?;

        let expected_triangle = row_num / 3;
        let expected_vert = row_num.wrapping_rem(3);

        if expected_triangle != row.triangle_index {
            return Err(Error::InvalidTriMesh);
        }

        if expected_vert != row.triangle_vertex_index {
            return Err(Error::InvalidTriMesh);
        }

        let texcoord = Point3::new(row.texture_u, row.texture_v, 0.0);
        csv_texcoords_3d.push(texcoord);
        let texcoord = Point2::new(row.texture_u, row.texture_v);
        csv_texcoords_2d.push(texcoord);

        let vertex = Point3::new(row.vertex_x, row.vertex_y, row.vertex_z);
        csv_worldcoords.push(vertex);

        let display_xy = Point3::new(row.display_x, row.display_y, 0.0);
        csv_displaycoords.push(display_xy);
    }

    let indices: Vec<usize> = (0..(csv_worldcoords.len())).collect();

    if !indices.len().is_multiple_of(3) {
        return Err(Error::RequiredTriMesh);
    }
    let indices: Vec<Point3<usize>> = indices
        .chunks(3)
        .map(|idxs| Point3::new(idxs[0], idxs[1], idxs[2]))
        .collect();

    // Step 3 - create a mesh that will allow looking up texcoord from display
    // coord, including interpolation. This means creating the display
    // coordinates for each vertex/texcoord in the geometry we just loaded.
    let orig_geom_worldcoords_mesh =
        ncollide3d::shape::TriMesh::<f64>::new(csv_worldcoords, indices, Some(csv_texcoords_2d));
    let geom = TriMeshGeom::new(&orig_geom_worldcoords_mesh, None)?;

    // let orig_geom_worldcoords_mesh: &ncollide3d::shape::TriMesh<_> = geom.worldcoords();
    let orig_geom_texcoords = geom.texcoords().points();

    fn drop_z(v3: &Point3<f64>) -> Point2<f64> {
        Point2::new(v3.x, v3.y)
    }

    // These are badly named because the "wcs" are actually the display XY
    // coords.
    let mut badly_named_wcs = Vec::new();
    let mut badly_named_indices = Vec::new();
    let mut badly_named_uvs = Vec::new();
    let mut bad_idx = 0;

    for tri_indices in orig_geom_worldcoords_mesh.indices().iter() {
        // because `impl<A, V> FromIterator<Option<A>> for Option<V>` exists,
        // `Vec<Option<Point3<_>>>` is here automatically `Option<Vec<Point3<_>>>`.
        let display_coords: Option<Vec<Point3<_>>> = tri_indices
            .iter()
            .map(|idx| {
                let real_texcoord = orig_geom_texcoords[*idx];
                // find the row in the CSV file with this texcoord

                get_idx(&csv_texcoords_3d, &real_texcoord).map(|i| csv_displaycoords[i])
            })
            .collect();

        // println!("display_coords {:?}", display_coords);
        if let Some(dcs) = display_coords {
            badly_named_wcs.push(dcs[0]);
            badly_named_wcs.push(dcs[1]);
            badly_named_wcs.push(dcs[2]);
            badly_named_uvs.push(drop_z(&orig_geom_texcoords[tri_indices[0]]));
            badly_named_uvs.push(drop_z(&orig_geom_texcoords[tri_indices[1]]));
            badly_named_uvs.push(drop_z(&orig_geom_texcoords[tri_indices[2]]));
            badly_named_indices.push(Point3::new(bad_idx, bad_idx + 1, bad_idx + 2));
            bad_idx += 3;
        }
    }

    // create a mesh
    let badly_named_wc_mesh = ncollide3d::shape::TriMesh::<f64>::new(
        badly_named_wcs,
        badly_named_indices,
        Some(badly_named_uvs),
    );
    // create our interpolation structure
    let badly_named_map = TriMeshGeom::new(&badly_named_wc_mesh, None)?;

    // Step 4 - loop through all display coordinates, interpolating to UV coords.
    let mut pixels: Vec<(f64, f64, f64)> = vec![(-1.0, -1.0, -1.0); width * height];

    use nalgebra::Vector3;
    let dir = Vector3::new(0.0, 0.0, 1.0);
    for row in 0..768 {
        for col in 0..1024 {
            let center = Point3::new(col as f64, row as f64, 0.0);
            let ray = ncollide3d::query::Ray::new(center, dir);

            match badly_named_map.intersect(&ray, Computable::TexCoords) {
                Computed::TexCoords(uv) => {
                    // can be nan
                    let (u, v) = uv;
                    if !u.is_nan() {
                        pixels[row * width + col] = (u, v, 1.0);
                    }
                }
            }
        }
    }

    if save_debug_images {
        let flat: Vec<f64> = pixels
            .iter()
            .flat_map(|px| vec![px.0, px.1, px.2])
            .collect();
        debug_image(&flat, height as u32, width as u32, 3, "out.jpg", None).unwrap();
    }

    let float_image = FloatImage {
        width,
        height,
        pixels,
    };

    let mut exr_writer = ExrWriter::default();
    // info!("saving EXR output file: {}", out_fname);
    exr_writer.update(&float_image, exr_comment);
    out_wtr.write_all(&exr_writer.buffer())?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct CommentParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<chrono::DateTime<chrono::Local>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_surface_model: Option<String>,
    display_width: usize,
    display_height: usize,
}

/// (advanced) get results of pinhole calibration as corresponding points
///
/// Projects all vertices in display surface model into display coordinates.
/// The written CSV file will have exactly the same triangle structure as the
/// original display surface model, which thus preserves the face indices.
/// Therefore, to remove particular points, set them to NaN rather than removing
/// them.
pub fn export_to_csv<W, TZ>(
    mut wtr: &mut W,
    cam: &braid_mvg::Camera<f64>,
    geom: &TriMeshGeom,
    created_at: Option<chrono::DateTime<TZ>>,
) -> Result<()>
where
    W: std::io::Write,
    TZ: chrono::TimeZone,
{
    writeln!(
        &mut wtr,
        "# This file contains FreemoVR calibration information."
    )?;
    writeln!(
        &mut wtr,
        "# Each group of three rows encodes a single triangle. \
        Therefore, some information is redundant (but otherwise it is difficult \
        to represent a triangle mesh in a conventional CSV file)."
    )?;

    let comment_params = CommentParams {
        created_at: created_at.map(|dt| dt.with_timezone(&chrono::Local)),
        display_surface_model: geom.original_fname().map(|x| x.to_string()),
        display_width: cam.width(),
        display_height: cam.height(),
    };

    let comment_params_buf = serde_json::to_string(&comment_params)?;
    writeln!(&mut wtr, "# params: {}", comment_params_buf)?;

    let mut wtr = csv::Writer::from_writer(&mut wtr);

    for (triangle_index, tri_idxs) in geom.worldcoords().indices().iter().enumerate() {
        for (triangle_vertex_index, tri_idx) in tri_idxs.iter().enumerate() {
            let wc = geom.worldcoords().points()[*tri_idx];
            let tc = geom.texcoords().points()[*tri_idx];

            let wc2 = braid_mvg::PointWorldFrame { coords: wc };
            let proj = cam.project_3d_to_pixel(&wc2);
            let wc2b: cam_geom::Points<_, _, nalgebra::U1, _> = (&wc2).into();
            let cam_frame = cam.extrinsics().world_to_camera(&wc2b);

            let row = crate::types::CompleteCorrespondance {
                triangle_index,
                triangle_vertex_index,
                display_x: proj.coords.x,
                display_y: proj.coords.y,
                display_depth: cam_frame.data[(0, 2)], // z coord of first (only) point
                texture_u: tc[0],
                texture_v: tc[1],
                vertex_x: wc[0],
                vertex_y: wc[1],
                vertex_z: wc[2],
            };
            wtr.serialize(row)?;
        }
    }

    Ok(())
}

/// find the index of a needle in a haystack
fn get_idx(
    haystack: &[nalgebra::geometry::Point3<f64>],
    needle: &nalgebra::geometry::Point3<f64>,
) -> Option<usize> {
    const EPSILON: f64 = 1e-2;
    const LARGE: f64 = 1e5;

    let acc = haystack
        .iter()
        .enumerate()
        .fold((0, LARGE), |acc, (this_idx, h)| {
            let (min_idx, min_val) = acc;
            let this_val = nalgebra::distance_squared(h, needle);
            if this_val < min_val {
                (this_idx, this_val)
            } else {
                (min_idx, min_val)
            }
        });
    let (min_idx, min_val) = acc;
    if min_val <= EPSILON {
        Some(min_idx)
    } else {
        None
    }
}

#[test]
fn test_get_index() {
    let haystack = vec![
        Point3::new(1.0, 2.0, 3.0),
        Point3::new(1.00001, 2.00001, 3.00001),
        Point3::new(1.000011, 2.000011, 3.000011),
        Point3::new(1.0000111, 2.0000111, 3.0000111),
        Point3::new(1.0000121, 2.0000121, 3.0000121),
        Point3::new(1.00001211, 2.00001211, 3.00001211),
    ];
    let needle = Point3::new(1.0000121, 2.0000121, 3.0000121);
    let opt_idx = get_idx(&haystack, &needle);
    assert_eq!(opt_idx, Some(4));

    let needle = Point3::new(0.0, 0.0, 0.0);
    let opt_idx = get_idx(&haystack, &needle);
    assert_eq!(opt_idx, None);
}
