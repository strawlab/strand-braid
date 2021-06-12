use crate::types::{
    Display, SimpleDisplay, SimpleUVCorrespondance, UVCorrespondance, VDispInfo, VirtualDisplay,
    VirtualDisplayName,
};
use crate::{DisplayGeometry, Result};
use nalgebra::geometry::{Point2, Point3};
use ncollide_geom::{mask_from_points, Mask};
use std::path::Path;

use crate::trimesh_ext::FaceIndices;

const DEFAULT_VDISP: &'static str = "fullscreen";

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct MultiDisplayInputFile {
    pub(crate) displays: Vec<MultiDisplayCalibrationFile>,
    pub(crate) final_size: FinalSize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct MultiDisplayCalibrationFile {
    pub(crate) calibration_file: std::path::PathBuf,
    pub(crate) x: usize,
    pub(crate) y: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct FinalSize {
    pub(crate) width: usize,
    pub(crate) height: usize,
}

#[derive(Debug, Clone)]
pub struct LoadedPinholeInputFile {
    pub loaded: PinholeInputFile,
    pub(crate) yaml_dir: std::path::PathBuf,
}

/// Can be used to run DLT
// TODO remove PinholeCal trait
// TODO return slices instead of Vecs
pub trait PinholeCalib {
    fn checkerboards(&self) -> Option<&[crate::Checkerboard]>;
    fn virtual_displays(&self) -> Vec<VirtualDisplay>;
    fn uv_display_points(&self) -> Vec<UVCorrespondance>;
    fn width(&self) -> usize;
    fn height(&self) -> usize;
}

/// Data for a single physical display with one or more virtual displays
///
/// For an example, see the file `tests/data/pinhole_wizard_sample.yaml`
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct PinholeInputFile {
    checkerboards: Option<Vec<crate::Checkerboard>>,
    uv_display_points: Vec<UVCorrespondance>,
    display: Display,
    pub(crate) geom: Geom,
}

/// Data for a single physical display with one fullscreen virtual display
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SimplePinholeInputFile {
    pub uv_display_points: Vec<SimpleUVCorrespondance>,
    pub display: SimpleDisplay,
    pub geom: Geom,
}

/// Data for a single physical display with trimesh geometry
#[derive(Debug, Serialize, Deserialize)]
pub struct SimplePinholeNoFile {
    pub display: crate::types::SimpleDisplay,
    pub uv_display_points: Vec<crate::types::SimpleUVCorrespondance>,
}

impl SimplePinholeInputFile {
    pub fn to_orig(self) -> PinholeInputFile {
        PinholeInputFile {
            checkerboards: None,
            uv_display_points: self
                .uv_display_points
                .into_iter()
                .map(|x| x.to_orig(DEFAULT_VDISP))
                .collect(),
            display: self.display.to_orig(DEFAULT_VDISP),
            geom: self.geom,
        }
    }
}

impl PinholeCalib for PinholeInputFile {
    fn checkerboards(&self) -> Option<&[crate::Checkerboard]> {
        match self.checkerboards {
            None => None,
            Some(ref v) => Some(v.as_slice()),
        }
    }

    fn virtual_displays(&self) -> Vec<VirtualDisplay> {
        self.display.virtual_displays.clone()
    }

    fn uv_display_points(&self) -> Vec<UVCorrespondance> {
        self.uv_display_points.clone()
    }

    fn width(&self) -> usize {
        self.display.width
    }

    fn height(&self) -> usize {
        self.display.height
    }
}

impl PinholeCalib for SimplePinholeNoFile {
    fn checkerboards(&self) -> Option<&[crate::Checkerboard]> {
        None
    }

    fn virtual_displays(&self) -> Vec<VirtualDisplay> {
        let display = self.display.clone().to_orig(DEFAULT_VDISP);
        display.virtual_displays
    }

    fn uv_display_points(&self) -> Vec<UVCorrespondance> {
        self.uv_display_points
            .clone()
            .into_iter()
            .map(|x| x.to_orig(DEFAULT_VDISP))
            .collect()
    }

    fn width(&self) -> usize {
        self.display.width
    }

    fn height(&self) -> usize {
        self.display.height
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct TexCoord {
    u: f64,
    v: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct WorldCoord {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields, tag = "model")]
pub enum Geom {
    #[serde(rename = "sphere")]
    Sphere(SphereGeom),
    #[serde(rename = "from_file")]
    FromFile(FromFileGeom),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SphereGeom {
    pub center: WorldCoord,
    pub radius: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct FromFileGeom {
    pub filename: String,
}

impl Geom {
    pub(crate) fn load_geom<P: AsRef<Path>>(
        &self,
        yaml_dir: P,
    ) -> Result<Box<dyn DisplayGeometry>> {
        match self {
            Geom::Sphere(sg) => Ok(Box::new(LoadedSphere::new(sg))),
            Geom::FromFile(_) => {
                let trimesh = self.as_trimesh(yaml_dir)?;
                Ok(Box::new(trimesh))
            }
        }
    }

    pub(crate) fn as_trimesh<P: AsRef<Path>>(&self, yaml_dir: P) -> Result<TriMeshGeom> {
        match self {
            Geom::Sphere(_) => Err(crate::error::Error::new(
                crate::error::ErrorKind::RequiredTriMesh,
            )),
            Geom::FromFile(ff) => {
                use failure::ResultExt;
                let mut obj_fname = yaml_dir.as_ref().to_path_buf();
                obj_fname.push(&ff.filename);

                let file = std::fs::File::open(&obj_fname).context(format!(
                    "loading geometry from file: {:?} (yaml dir is \"{}\"",
                    ff.filename,
                    yaml_dir.as_ref().display()
                ))?;
                parse_obj_from_reader(file, Some(&ff.filename))
            }
        }
    }
}

pub fn parse_obj_from_reader<R: std::io::Read>(
    mut file: R,
    fname: Option<&str>,
) -> Result<TriMeshGeom> {
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut buf)?;

    let mut objects = simple_obj_parse::obj_parse(&buf)?;
    if objects.len() != 1 {
        use crate::error::ErrorKind::ObjMustHaveExactlyOneObject;
        return Err(crate::error::Error::new(ObjMustHaveExactlyOneObject(
            objects.len(),
        )));
    }

    let obj = objects.remove(0);
    let (_name, mesh) = obj;

    Ok(TriMeshGeom::new(&mesh, fname.map(|x| x.to_string()))?)
}

struct LoadedSphere {
    center: Point3<f64>,
    radius: f64,
    bounding_sphere: ncollide3d::bounding_volume::BoundingSphere<f64>,
}

impl LoadedSphere {
    fn new(sg: &SphereGeom) -> Self {
        let c = &sg.center;
        let center = Point3::new(c.x, c.y, c.z);
        let bounding_sphere = ncollide3d::bounding_volume::BoundingSphere::new(center, sg.radius);
        Self {
            center,
            radius: sg.radius,
            bounding_sphere,
        }
    }
}

impl DisplayGeometry for LoadedSphere {
    fn texcoord2worldcoord(&self, tc: &Point2<f64>) -> Option<Point3<f64>> {
        // see freemovr_enginge: DisplaySurfaceGeometry.cpp and simple_geom.py
        let frac_az = tc[0];
        let frac_el = tc[1];

        let az = frac_az * 2.0 * std::f64::consts::PI;
        let el = frac_el * std::f64::consts::PI - std::f64::consts::PI / 2.0;

        let (sa, ca) = az.sin_cos();

        let (se, ce) = el.sin_cos();

        let r = self.radius;

        let x = r * ca * ce;
        let y = r * sa * ce;
        let z = r * se;

        Some(Point3::new(
            x + self.center[0],
            y + self.center[1],
            z + self.center[2],
        ))
    }

    fn worldcoord2texcoord(&self, surface_pt: &Point3<f64>) -> Option<Point2<f64>> {
        let rel = (surface_pt - self.center) / self.radius;
        let el = rel.z.asin(); // range [-pi/2, pi/2]
        let az = rel.y.atan2(rel.x); // range [-pi,pi]

        let frac_el = (el + std::f64::consts::PI / 2.0) / std::f64::consts::PI;
        let frac_az = az / (2.0 * std::f64::consts::PI);

        // deal with wraparound on frac_az
        let frac_az = if frac_az < 0.0 {
            frac_az + 1.0
        } else {
            frac_az
        };
        Some(Point2::new(frac_az, frac_el))
    }

    fn ncollide_shape(&self) -> &dyn ncollide3d::query::RayCast<f64> {
        &self.bounding_sphere
    }
}

// the Z coord of TriMesh is 0
fn get_uvs_trimesh(
    mesh: &ncollide3d::shape::TriMesh<f64>,
) -> Option<ncollide3d::shape::TriMesh<f64>> {
    if let Some(ref uvs) = mesh.uvs() {
        let indices = mesh.indices();
        let coords = uvs
            .iter()
            .map(|uv| Point3::new(uv[0], uv[1], 0.0))
            .collect();
        Some(ncollide3d::shape::TriMesh::<f64>::new(
            coords, indices, None,
        ))
    } else {
        None
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TriMeshGeom {
    original_fname: Option<String>,

    /// 3D world coordinates, shared indices with `uvs`
    worldcoords: ncollide3d::shape::TriMesh<f64>,

    /// 3rd coord ("z") is 0.0, shared indices with `worldcoords`
    uvs: ncollide3d::shape::TriMesh<f64>,
}

impl std::fmt::Debug for TriMeshGeom {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "TriMeshGeom {{ original_fname: {:?} }}",
            self.original_fname
        )
    }
}

impl TriMeshGeom {
    pub fn new(
        mesh: &ncollide3d::shape::TriMesh<f64>,
        original_fname: Option<String>,
    ) -> Result<Self> {
        let uvs = match get_uvs_trimesh(&mesh) {
            Some(uvs) => uvs,
            None => {
                use crate::error::ErrorKind::ObjHasNoTextureCoords;
                return Err(crate::Error::new(ObjHasNoTextureCoords));
            }
        };
        Ok(Self {
            original_fname,
            worldcoords: mesh.clone(),
            uvs,
        })
    }

    pub fn worldcoords(&self) -> &ncollide3d::shape::TriMesh<f64> {
        &self.worldcoords
    }

    pub fn texcoords(&self) -> &ncollide3d::shape::TriMesh<f64> {
        &self.uvs
    }

    pub fn original_fname(&self) -> Option<&str> {
        // see https://stackoverflow.com/a/31234028/1633026
        self.original_fname.as_ref().map(|x| &**x)
    }
}

impl DisplayGeometry for TriMeshGeom {
    fn texcoord2worldcoord(&self, tc: &Point2<f64>) -> Option<Point3<f64>> {
        use ncollide3d::query::PointQueryWithLocation;
        use ncollide3d::shape::TrianglePointLocation;

        let point = Point3::new(tc[0], tc[1], 0.0);

        use nalgebra::geometry::Isometry3;

        // tri_idx is the number of the triangle
        let (pp, (tri_idx, loc)) =
            self.uvs
                .project_point_with_location(&Isometry3::identity(), &point, true);

        if !pp.is_inside {
            return None;
        }

        // tri_indices are the indices of into e.g. uvs for this triangle
        let tri_indices = self.uvs.indices()[tri_idx];

        let wc = match loc {
            TrianglePointLocation::OnVertex(i) => {
                let idx = tri_indices[i];
                self.worldcoords.points()[idx]
            }
            TrianglePointLocation::OnEdge(edge_num, bcoords) => {
                let (idx0, idx1) = match edge_num {
                    0 => unimplemented!(),
                    1 => (tri_indices[1], tri_indices[2]),
                    2 => (tri_indices[0], tri_indices[2]),
                    _ => panic!("triangle with >3 edges?"),
                };
                let vertices = self.worldcoords.points();
                let worldcoord =
                    vertices[idx0].coords * bcoords[0] + vertices[idx1].coords * bcoords[1];
                worldcoord.into()
            }
            TrianglePointLocation::OnFace(_face_idx, bcoords) => {
                let vertices = self.worldcoords.points();

                let worldcoord = vertices[tri_indices[0]].coords * bcoords[0]
                    + vertices[tri_indices[1]].coords * bcoords[1]
                    + vertices[tri_indices[2]].coords * bcoords[2];
                worldcoord.into()
            }
            TrianglePointLocation::OnSolid => {
                panic!("impossible: TriMesh OnSolid");
            }
        };
        Some(wc)
    }

    fn worldcoord2texcoord(&self, surface_pt: &Point3<f64>) -> Option<Point2<f64>> {
        use crate::trimesh_ext::UvPosition;
        use nalgebra::geometry::Isometry3;

        self.worldcoords
            .project_point_to_uv(&Isometry3::identity(), &surface_pt, false)
    }

    fn ncollide_shape(&self) -> &dyn ncollide3d::query::RayCast<f64> {
        &self.worldcoords
    }
}

/// Solve (using DLT) the pinhole camera for each virtual display in input data
pub fn solve_no_distortion_display_camera<D: PinholeCalib>(
    data: &D,
    geom: &dyn DisplayGeometry,
    epsilon: f64,
) -> Result<Vec<(VirtualDisplayName, mvg::Camera<f64>)>> {
    // TODO: use checkerboards if available and share intrinsics across all virtual displays
    if data.checkerboards().is_some() {
        warn!("checkerboard data present, but using with no distortion.");
    }

    // FIXME: should compute intrinsics once for all virtual displays, even in
    // case where we are not using checkerboards to calculate distortions.
    let mut result = Vec::new();

    for vdisp in data.virtual_displays().iter() {
        let uv_display_points = data.uv_display_points();
        let this_vdisp_points = uv_display_points
            .iter()
            .filter(|row| row.virtual_display == vdisp.id);

        let this_vdisp_corr: Result<Vec<dlt::CorrespondingPoint<f64>>> = this_vdisp_points
            .map(|row| {
                let tc = Point2::new(row.texture_u, row.texture_v);
                let wc = geom.texcoord2worldcoord(&tc).ok_or_else(|| {
                    crate::error::Error::new(crate::error::ErrorKind::InvalidTexCoord)
                })?;
                let ic = [row.display_x, row.display_y];

                let corr = dlt::CorrespondingPoint {
                    object_point: [wc.x, wc.y, wc.z],
                    image_point: ic,
                };

                Ok(corr)
            })
            .collect();

        let this_vdisp_corr = this_vdisp_corr?;

        info!(
            "computing for virtual display: {} with {} points",
            vdisp.id.0,
            this_vdisp_corr.len()
        );

        use crate::error::ErrorKind::SvdError;

        let dlt_pmat = dlt::dlt_corresponding(&this_vdisp_corr, epsilon)
            .map_err(|e| crate::error::Error::new(SvdError(e)))?;

        // println!("pmat: {}", pretty_print_nalgebra::pretty_print!(&dlt_pmat));

        let cam1 = mvg::Camera::from_pmat(data.width(), data.height(), &dlt_pmat)?;
        let cam2 = cam1.flip().expect("flip camera");

        // take whichever camera points towards objects
        let cam = if mean_forward(&cam1, &this_vdisp_corr) > mean_forward(&cam2, &this_vdisp_corr) {
            cam1
        } else {
            cam2
        };
        result.push((vdisp.id.clone(), cam));
    }
    Ok(result)
}

fn mean_forward(cam: &mvg::Camera<f64>, pts: &[dlt::CorrespondingPoint<f64>]) -> f64 {
    use mvg::PointWorldFrame;
    let mut accum = 0.0;
    for pt in pts {
        let o = pt.object_point;
        let world_pt = PointWorldFrame {
            coords: Point3::from_slice(&o),
        };

        let wc2b: cam_geom::Points<_, _, nalgebra::U1, _> = (&world_pt).into();
        let cam_pt = cam.extrinsics().world_to_camera(&wc2b);
        let cam_dist = cam_pt.data[(0, 2)];
        accum += cam_dist;
    }
    accum / pts.len() as f64
}

pub fn compute_mask<D: PinholeCalib>(data: &D, id: &VirtualDisplayName) -> Result<Mask> {
    for vdisp in data.virtual_displays().iter() {
        // vdisp is &VirtualDisplay
        if &vdisp.id == id {
            let points: Vec<_> = vdisp
                .viewport
                .iter()
                .map(|(x, y)| (*x as f64, *y as f64))
                .collect();
            let compound = mask_from_points(&points);
            return Ok(compound);
        }
    }
    use crate::error::ErrorKind::VirtualDisplayNotFound;
    Err(crate::error::Error::new(VirtualDisplayNotFound))
}

pub fn merge_vdisps<D: PinholeCalib>(
    data: &D,
    vdisp_data: &[&VDispInfo],
    show_mask: bool,
) -> Vec<f64> {
    use ncollide2d::query::point_query::PointQuery;

    let mut im_data: Vec<f64> = vec![-1.0; data.width() * data.height() * 3];
    let m = nalgebra::geometry::Isometry::identity();
    let mut n_overlap = 0;

    for row in 0..data.height() {
        for col in 0..data.width() {
            let start = (row * data.width() + col) * 3;
            let start_dest = (row * data.width() + col) * 2;
            let mut found = false;
            for (vdisp_num, (mask, vdisp_data, nchan)) in vdisp_data.iter().enumerate() {
                debug_assert!(*nchan == 2);
                let cur_pos = nalgebra::geometry::Point2::new(col as f64, row as f64);
                if mask.distance_to_point(&m, &cur_pos, true) < 1.0 {
                    if found {
                        n_overlap += 1;
                    }
                    found = true;
                    // now we are in mask
                    // blue channel = blending amount: 0.0 do not show, 1.0 max

                    let u = vdisp_data[start_dest];
                    let v = vdisp_data[start_dest + 1];
                    let (r, g, b) = if u.is_nan() {
                        if im_data[start + 2] == 0.0 {
                            // In case this vdisp doesn't have a good pixel, we
                            // keep what is already here if it is good.
                            (im_data[start], im_data[start + 1], 1.0)
                        } else {
                            (-2.0, -2.0, -2.0)
                        }
                    } else {
                        (u, v, 1.0)
                    };
                    im_data[start] = r;
                    im_data[start + 1] = g;
                    im_data[start + 2] = b;

                    if show_mask {
                        if vdisp_num == 0 {
                            im_data[start] = 1.0;
                            im_data[start + 1] = 0.0;
                            im_data[start + 2] = 0.0;
                        }
                        if vdisp_num == 1 {
                            im_data[start] = 0.0;
                            im_data[start + 1] = 1.0;
                            im_data[start + 2] = 0.0;
                        }
                        if vdisp_num == 2 {
                            im_data[start] = 0.0;
                            im_data[start + 1] = 0.0;
                            im_data[start + 2] = 1.0;
                        }
                    }
                }
            }
        }
    }
    if n_overlap > 0 {
        error!("Overlapping viewports. Some virtual displays are obscured by others.");
    }
    im_data
}

#[cfg(test)]
mod tests {

    use crate::pinhole_wizard_yaml_support::*;
    use crate::DisplayGeometry;
    use nalgebra::geometry::Point2;

    fn check_texcoord_worldcoord_roundtrip(geom: &dyn DisplayGeometry, uvs: &[Point2<f64>]) {
        for uv in uvs.iter() {
            let wc = geom.texcoord2worldcoord(uv);
            if let Some(wc) = wc {
                let uv2 = geom.worldcoord2texcoord(&wc).unwrap();
                let eps = 1e-10;
                println!("uv {:?} -> wc {:?} -> uv2 {:?}", uv, wc, uv2);
                assert!((uv[0] - uv2[0]).abs() < eps);
                assert!((uv[1] - uv2[1]).abs() < eps);
            }
        }
    }

    #[test]
    fn cyl_from_file_texcoord_worldcoord_roundtrip() {
        let buf = include_bytes!("../tests/data/cylinder.obj");
        let objects = simple_obj_parse::obj_parse(buf).unwrap();
        assert_eq!(objects.len(), 1);

        let mut uvs = Vec::new();
        // these coords avoid branch cuts at U=0.0 and U=1.0
        for u in &[0.01f64, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.99] {
            for v in &[0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0] {
                let uv = Point2::new(*u, *v);
                uvs.push(uv);
            }
        }

        for obj in objects {
            let (_name, mesh) = obj;
            let geom = TriMeshGeom::new(&mesh, None).unwrap();
            check_texcoord_worldcoord_roundtrip(&geom, &uvs);
        }
    }

    // tex coords are not entire 0-1 range for u or v
    static SQUARE: &'static [u8] = b"
    v 0 1 0
    v 0 0 0
    v 1 0 0
    v 1 1 0
    vt 0 0.5
    vt 0 0
    vt 0.5 0
    vt 0.5 0.5
    f 1/1 2/2 3/3 4/4
    ";

    #[test]
    fn square_texcoord_worldcoord_roundtrip() {
        let objects = simple_obj_parse::obj_parse(SQUARE).unwrap();
        assert_eq!(objects.len(), 1);

        let mut uvs = Vec::new();
        for u in &[0.00f64, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0] {
            for v in &[0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0] {
                let uv = Point2::new(*u, *v);
                uvs.push(uv);
            }
        }

        for obj in objects {
            let (_name, mesh) = obj;
            let geom = TriMeshGeom::new(&mesh, None).unwrap();
            check_texcoord_worldcoord_roundtrip(&geom, &uvs);
        }
    }

    #[test]
    fn sphere_texcoord_worldcoord_roundtrip() {
        let sg = SphereGeom {
            center: WorldCoord {
                x: 0.1,
                y: 2.3,
                z: 4.5,
            },
            radius: 1.234,
        };
        let geom = LoadedSphere::new(&sg);

        let mut uvs = Vec::new();
        // these coords avoid branch cuts at U=1.0 and poles at V=0 and V=1
        for u in &[0.0f64, 0.1f64, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.99] {
            for v in &[0.01, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 0.99] {
                let uv = Point2::new(*u, *v);
                uvs.push(uv);
            }
        }

        check_texcoord_worldcoord_roundtrip(&geom, &uvs);
    }

    #[test]
    fn trimesh_dense_interp() {
        use crate::{Computable, Computed};
        use nalgebra::Vector3;

        let coords: Vec<Point3<f64>> = vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ];
        let uvs: Vec<Point2<f64>> = vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
        ];
        let indices: Vec<Point3<usize>> = vec![Point3::new(0, 1, 2)];

        let mesh = ncollide3d::shape::TriMesh::<f64>::new(coords, indices, Some(uvs));
        let geom = TriMeshGeom::new(&mesh, None).unwrap();

        // Test roundtrip tc->wc->tc
        let uv = Point2::new(0.5, 0.5);
        let wc = geom.texcoord2worldcoord(&uv).unwrap();
        let uv2 = geom.worldcoord2texcoord(&wc).unwrap();

        let eps = 1e-10;
        assert!((uv[0] - uv2[0]).abs() < eps);
        assert!((uv[1] - uv2[1]).abs() < eps);

        // Test ray -> uv

        let dir = Vector3::new(0.0, 0.0, -1.0);
        let center = Point3::new(0.5, 0.5, 1.0);
        let ray = ncollide3d::query::Ray::new(center, dir);
        let uv = geom.intersect(&ray, Computable::TexCoords);

        match uv {
            Computed::TexCoords(uv) => {
                assert!((uv.0 - center[0]).abs() < eps);
                assert!((uv.1 - center[1]).abs() < eps);
            }
        }
    }

    // TODO: test yaml file which specifies display geometry in .obj file

    #[test]
    fn obj_test() {
        use crate::{compute_image_for_camera_view, Computable};
        use nalgebra::Vector3;

        let camcenter = Vector3::new(0.5, 0.5, 10.0);
        let lookat = Vector3::new(0.5, 0.5, 0.0);
        let up = nalgebra::core::Unit::new_normalize(Vector3::new(0.0, 1.0, 0.0));
        let extrinsics = cam_geom::ExtrinsicParameters::from_view(&camcenter, &lookat, &up);

        let cx = 640.0;
        let cy = 512.0;
        let params = cam_geom::PerspectiveParams {
            fx: 100.0,
            fy: 100.0,
            skew: 0.0,
            cx,
            cy,
        };
        let intrinsics: cam_geom::IntrinsicParametersPerspective<_> = params.into();
        let cam = mvg::Camera::new(
            2 * cx as usize,
            2 * cy as usize,
            extrinsics,
            intrinsics.into(),
        )
        .unwrap();

        let obj = simple_obj_parse::obj_parse(SQUARE).unwrap().remove(0);
        let (_name, mesh) = obj;
        let geom = TriMeshGeom::new(&mesh, None).unwrap();

        let (w, h) = (cam.width() as f64, cam.height() as f64);
        let viewport = vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
        let mask = mask_from_points(&viewport);

        let (texcoords, nchan) =
            compute_image_for_camera_view(&cam, Computable::TexCoords, &geom, &mask).unwrap();

        crate::debug_image(
            &texcoords,
            cam.height() as u32,
            cam.width() as u32,
            nchan,
            "square.jpg",
            None,
        )
        .unwrap();
    }

    #[test]
    fn test_trimesh_geom() {
        // When I wrote TriMeshGeom, I falsely assumed the texture coordinate
        // indicies and the vertex indices would be 1:1. This is not true and
        // here this is tested.
        let buf = include_bytes!("../tests/data/tetrahedron/tetrahedron.obj");
        let mut buf_ref: &[u8] = buf.as_ref();
        let trimesh = parse_obj_from_reader(&mut buf_ref, None).unwrap();

        let texcoords = vec![
            Point2::new(0.1, 0.1),
            Point2::new(0.25, 0.25),
            Point2::new(0.25, 0.75),
            Point2::new(0.5, 0.25),
            Point2::new(0.8, 0.25),
            Point2::new(0.75, 0.75),
        ];

        let vertices = vec![
            None,
            Some(Point3::new(0.25, 0.25, 0.5)),
            Some(Point3::new(0.25, -0.25, -0.5)),
            Some(Point3::new(-0.5, -0.5, 0.0)),
            Some(Point3::new(-0.6, 0.6, 0.2)),
            None,
        ];

        for (input, expected) in texcoords.iter().zip(vertices.iter()) {
            println!("\ninput {:?}", input);
            let actual = trimesh.texcoord2worldcoord(input);
            println!("expected {:?}", expected);
            println!("actual {:?}", actual);
            if let Some(expected) = expected {
                let actual = actual.unwrap();
                approx::assert_relative_eq!(actual, expected, epsilon = 1e-0);
            } else {
                assert!(actual.is_none())
            }
        }

        for (input, expected) in vertices.iter().zip(texcoords.iter()) {
            if let Some(input) = input {
                println!("\ninput {:?}", input);
                let actual = trimesh.worldcoord2texcoord(input);
                println!("expected {:?}", expected);
                println!("actual {:?}", actual);
                let actual = actual.unwrap();
                approx::assert_relative_eq!(actual, expected, epsilon = 1e-0);
            }
        }
    }
}
