#![recursion_limit = "2048"]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;

use nalgebra::geometry::{Point2, Point3};

use yew::prelude::*;

use ads_webasm::components::{Button, CsvData, CsvDataField, MaybeCsvData};

#[derive(Debug, Serialize, Deserialize)]
pub struct MyError {
    pub msg: String,
}

impl From<std::io::Error> for MyError {
    fn from(orig: std::io::Error) -> MyError {
        MyError {
            msg: format!("std::io::Error: {}", orig),
        }
    }
}

impl From<serde_yaml::Error> for MyError {
    fn from(orig: serde_yaml::Error) -> MyError {
        MyError {
            msg: format!("serde_yaml::Error: {}", orig),
        }
    }
}

impl From<mvg::MvgError> for MyError {
    fn from(orig: mvg::MvgError) -> MyError {
        MyError {
            msg: format!("mvg::MvgError: {}", orig),
        }
    }
}

impl std::fmt::Display for MyError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fiducial3DCoords {
    id: u32,
    x: f64,
    y: f64,
    z: f64,
}

// The center pixel of the detection is (h02,h12)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DetectionSerializer {
    frame: usize,
    time_microseconds: i64,
    id: i32,
    hamming: i32,
    decision_margin: f32,
    h00: f64,
    h01: f64,
    h02: f64,
    h10: f64,
    h11: f64,
    h12: f64,
    h20: f64,
    h21: f64,
    // no h22 because it is always 1.0
    family: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AprilConfig {
    pub created_at: chrono::DateTime<chrono::Local>,
    pub camera_name: String,
    pub camera_width_pixels: usize,
    pub camera_height_pixels: usize,
}

pub fn get_cfg<R: std::io::Read>(rdr: R) -> Result<AprilConfig, MyError> {
    use std::io::BufRead;
    let buf_reader = std::io::BufReader::new(rdr);

    enum ReaderState {
        JustStarted,
        InYaml(Vec<String>),
    }

    let mut state = ReaderState::JustStarted;
    for line in buf_reader.lines() {
        let line = line?;
        match state {
            ReaderState::JustStarted => {
                if !line.starts_with("# ") {
                    return Err(MyError {
                        msg: "File did not start with comment '# '".into(),
                    });
                }
                if line == "# -- start of yaml config --" {
                    state = ReaderState::InYaml(Vec::new());
                }
            }
            ReaderState::InYaml(ref mut yaml_lines) => {
                if line.starts_with("# ") {
                    if line == "# -- end of yaml config --" {
                        break;
                    } else {
                        let cleaned: &str = &line[2..];
                        yaml_lines.push(cleaned.to_string());
                    }
                } else {
                    return Err(MyError {
                        msg: "YAML section started but never finished".into(),
                    });
                }
            }
        }
    }
    if let ReaderState::InYaml(yaml_lines) = state {
        let mut yaml_buf: Vec<u8> = Vec::new();
        for line in yaml_lines {
            yaml_buf.extend(line.as_bytes());
            yaml_buf.push(b'\n');
        }
        let cfg: AprilConfig = serde_yaml::from_reader(yaml_buf.as_slice())?;
        Ok(cfg)
    } else {
        Err(MyError {
            msg: "YAML section started but never finished".into(),
        })
    }
}

pub struct Model {
    link: ComponentLink<Self>,
    fiducial_3d_coords: MaybeCsvData<Fiducial3DCoords>,
    per_camera_2d: BTreeMap<String, (AprilConfig, CsvData<DetectionSerializer>)>,
    computed_xml_calibration: Option<CalibrationResult>,
}

pub enum Msg {
    Fiducial3dCoordsData(MaybeCsvData<Fiducial3DCoords>),
    DetectionSerializerData(MaybeCsvData<DetectionSerializer>),
    RemoveCamera(String),
    ComputeCal,
    DownloadCal,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
            fiducial_3d_coords: MaybeCsvData::Empty,
            per_camera_2d: BTreeMap::new(),
            computed_xml_calibration: None,
        }
    }
    fn change(&mut self, _props: ()) -> ShouldRender {
        false
    }
    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Fiducial3dCoordsData(csv_file) => {
                self.computed_xml_calibration = None;
                self.fiducial_3d_coords = csv_file;
            }
            Msg::DetectionSerializerData(csv_file) => match csv_file {
                MaybeCsvData::Valid(csv_data) => {
                    let raw_buf: &[u8] = csv_data.raw_buf();
                    match get_cfg(raw_buf) {
                        Ok(cfg) => {
                            self.per_camera_2d
                                .insert(cfg.camera_name.clone(), (cfg, csv_data));
                        }
                        Err(e) => {
                            log::error!("failed getting camera name: {}", e);
                        }
                    }
                }
                _ => {
                    log::error!("CSV error: empty file or failed parsing");
                }
            },
            Msg::RemoveCamera(cam_name) => {
                self.per_camera_2d.remove(&cam_name);
            }
            Msg::ComputeCal => match self.get_cal_data() {
                Ok(src_data) => {
                    match do_calibrate_system(&src_data) {
                        Ok(cal) => {
                            self.computed_xml_calibration = Some(cal);
                        }
                        Err(e) => {
                            log::error!("Error performing calibration: {}", e);
                        }
                    };
                }
                Err(e) => {
                    log::error!("could not get calibration data: {:?}", e);
                }
            },
            Msg::DownloadCal => {
                if let Some(ref cal) = self.computed_xml_calibration {
                    let buf = cal.to_flydra_xml().unwrap();
                    download_file(&buf, "braid-calibration.xml"); // TODO: set filename to date/time?
                }
            }
        }
        true
    }

    fn view(&self) -> Html {
        let fiducial_3d_coords_file_state = format!("{}", self.fiducial_3d_coords);

        let compute_xml_title = format!(
            "Compute XML calibration with {} cameras",
            self.per_camera_2d.len()
        );

        let download_xml_str = if self.can_compute_xml_calibration() {
            ""
        } else {
            "Calibration not ready, cannot download."
        };

        html! {
            <div id="page-container">
            <div id="content-wrap">
            <h1>{"Braid April Tag Calibration Tool"}</h1>
            <h3>{"by Andrew Straw, Straw Lab, University of Freiburg, Germany"}</h3>
            <p>{"This page computes a "}<a href="https://strawlab.org/braid/">{"Braid"}</a>
               {" calibration based on April Tag fiducial marker detection data. "}
               {"The source code for this page may be found "}
               <a href="https://github.com/strawlab/strand-braid/tree/main/braid-april-cal-webapp">
               {"here"}</a>{"."}</p>
            <h2>{"Input: 3D coordinates of April Tag fiducial markers"}</h2>
            <p>{"The file must be a CSV file with columns: id, x, y, z."}</p>
            <label class=classes!("btn", "custom-file-upload")>
                {"Upload a 3D coordinate CSV file."}
                <CsvDataField<Fiducial3DCoords>
                    onfile=self.link.callback(Msg::Fiducial3dCoordsData)
                    />
            </label>
            <p>
                { &fiducial_3d_coords_file_state }
            </p>

            <h2>{"Input: Automatically detected camera coordinates of April Tag fiducial markers"}</h2>
            <p>{"The file must be a CSV file saved by the April Tag detector of Strand Cam. (Columns include hamming and detection_margin.)"}</p>
            <label class=classes!("btn", "custom-file-upload")>
                {"Upload a camera coordinate CSV file."}
                <CsvDataField<DetectionSerializer>
                    onfile=self.link.callback(Msg::DetectionSerializerData)
                    />
            </label>
            {self.view_camera_data()}

            <h2>{"Output"}</h2>
            <div>
                {download_xml_str}
            </div>
            <Button
                title=compute_xml_title
                onsignal=self.link.callback(|()| Msg::ComputeCal)
                disabled=!self.can_compute_xml_calibration()
                />
            <Button
                title="Download XML calibration"
                onsignal=self.link.callback(|()| Msg::DownloadCal)
                disabled=self.computed_xml_calibration.is_none()
                />
            <footer id="footer">{format!("Tool date: {} (revision {})",
                env!("GIT_DATE"),
                env!("GIT_HASH"))}
            </footer>
        </div>
        </div>
        }
    }
}

impl Model {
    fn can_compute_xml_calibration(&self) -> bool {
        let has_3d = if let MaybeCsvData::Valid(_) = &self.fiducial_3d_coords {
            true
        } else {
            false
        };
        !self.per_camera_2d.is_empty() && has_3d
    }
    fn view_camera_data(&self) -> Html {
        if self.per_camera_2d.is_empty() {
            return html! {
                <p>{"No camera data loaded"}</p>
            };
        }
        let items: Vec<Html> = self
            .per_camera_2d
            .iter()
            .map(|(cam_name, all_csv_data)| {
                let (_cfg, csv_data) = all_csv_data;
                let cam_name: String = cam_name.clone();
                html! {
                    <li>
                        {format!("{}: {} detections (file: {})",cam_name,csv_data.rows().len(), csv_data.filename())}
                        <Button
                            title="Remove"
                            onsignal=self.link.callback(move |()| Msg::RemoveCamera(cam_name.clone()))
                        />
                    </li>
                }
            })
            .collect();
        html! {
            <ul>
                {items}
            </ul>
        }
    }

    fn get_cal_data(&self) -> Result<CalData, MyError> {
        if !self.can_compute_xml_calibration() {
            return Err(MyError {
                msg: "insufficient data loaded to compute calibration".into(),
            });
        }
        let fiducial_3d_coords = if let MaybeCsvData::Valid(csv_data) = &self.fiducial_3d_coords {
            // Make a copy of the data.
            csv_data.rows().to_vec()
        } else {
            // we just guaranteed that we have this data.
            panic!("unreachable");
        };
        let per_camera_2d = self
            .per_camera_2d
            .iter()
            .map(|(cam_name, all_data)| {
                (
                    cam_name.clone(),
                    (all_data.0.clone(), all_data.1.rows().to_vec()),
                )
            })
            .collect();
        Ok(CalData {
            fiducial_3d_coords,
            per_camera_2d,
        })
    }
}

fn download_file(orig_buf: &[u8], filename: &str) {
    let mime_type = "application/octet-stream";
    let b = js_sys::Uint8Array::new(&unsafe { js_sys::Uint8Array::view(orig_buf) }.into());
    let array = js_sys::Array::new();
    array.push(&b.buffer());

    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(
        &array,
        web_sys::BlobPropertyBag::new().type_(mime_type),
    )
    .unwrap();
    let data_url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
    let document = web_sys::window().unwrap().document().unwrap();
    let anchor = document
        .create_element("a")
        .unwrap()
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .unwrap();

    anchor.set_href(&data_url);
    anchor.set_download(filename);
    anchor.set_target("_blank");

    anchor.style().set_property("display", "none").unwrap();
    let body = document.body().unwrap();
    body.append_child(&anchor).unwrap();

    anchor.click();

    body.remove_child(&anchor).unwrap();
    web_sys::Url::revoke_object_url(&data_url).unwrap();
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CalData {
    pub fiducial_3d_coords: Vec<Fiducial3DCoords>,
    pub per_camera_2d: BTreeMap<String, (AprilConfig, Vec<DetectionSerializer>)>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CalibrationResult {
    pub cam_system: mvg::MultiCameraSystem<f64>,
    pub mean_reproj_dist: BTreeMap<String, f64>,
}

impl CalibrationResult {
    fn to_flydra_xml(&self) -> Result<Vec<u8>, MyError> {
        let flydra_cal =
            flydra_mvg::FlydraMultiCameraSystem::<f64>::from_system(self.cam_system.clone(), None);

        let mut xml_buf: Vec<u8> = Vec::new();
        flydra_cal
            .to_flydra_xml(&mut xml_buf)
            .expect("to_flydra_xml");
        Ok(xml_buf)
    }
}

pub fn do_calibrate_system(src_data: &CalData) -> Result<CalibrationResult, MyError> {
    let mut object_points = BTreeMap::new();
    for row in src_data.fiducial_3d_coords.iter() {
        if object_points
            .insert(row.id, [row.x, row.y, row.z])
            .is_some()
        {
            return Err(MyError {
                msg: format!("multiple entries for ID {} in 3D data file", row.id),
            });
        }
    }

    let mut mean_reproj_dist = BTreeMap::new();
    let mut cams = BTreeMap::new();

    for (cam_name, all_cam_data) in src_data.per_camera_2d.iter() {
        let (cfg, cam_data) = all_cam_data;
        assert_eq!(&cfg.camera_name, cam_name);

        // Iterate through all rows of detection data to collect all detections
        // per marker.
        let mut uv_per_id = BTreeMap::new();
        for row in cam_data {
            uv_per_id
                .entry(row.id as u32)
                .or_insert_with(Vec::new)
                .push((row.h02, row.h12)); // The (x,y) pixel coord of detection.
        }

        let mut points = Vec::new();
        for (id, uv) in uv_per_id.iter() {
            // calculate mean (u,v) position
            let (sumu, sumv) = uv.iter().fold((0.0, 0.0), |accum, elem| {
                (accum.0 + elem.0, accum.1 + elem.1)
            });
            let u = sumu / uv.len() as f64;
            let v = sumv / uv.len() as f64;

            if let Some(from_csv) = object_points.get(id) {
                let object_point = *from_csv;
                let pt = dlt::CorrespondingPoint {
                    object_point,
                    image_point: [u, v],
                };
                points.push(pt);
            }
        }

        // Compute calibration here
        let epsilon = 1e-10;
        let dlt_pmat =
            dlt::dlt_corresponding(&points, epsilon).map_err(|msg| MyError { msg: msg.into() })?;

        let cam1 =
            mvg::Camera::from_pmat(cfg.camera_width_pixels, cfg.camera_height_pixels, &dlt_pmat)?;
        let cam2 = cam1.flip().expect("flip camera");

        // take whichever camera points towards objects
        let cam = if mean_forward(&cam1, &points) > mean_forward(&cam2, &points) {
            cam1
        } else {
            cam2
        };

        // Compute reprojection distance.
        let dists: Vec<f64> = points
            .iter()
            .map(|pt| {
                let world_pt = mvg::PointWorldFrame {
                    coords: Point3::from_slice(&pt.object_point),
                };
                let image_point = Point2::from_slice(&pt.image_point);
                let projected_pixel = cam.project_3d_to_distorted_pixel(&world_pt);
                nalgebra::distance(&projected_pixel.coords, &image_point)
            })
            .collect();

        if dists.is_empty() {
            return Err(MyError{msg:format!("Camera {}: could not compute reprojection distance. Are there marker detections also in 3D data?", cam_name)});
        }
        let sum_dist = dists.iter().fold(0.0, |accum, el| accum + el);
        let mean_dist = sum_dist / dists.len() as f64;

        cams.insert(cam_name.clone(), cam);
        mean_reproj_dist.insert(cam_name.clone(), mean_dist);
    }

    let cam_system = mvg::MultiCameraSystem::new(cams);

    Ok(CalibrationResult {
        cam_system,
        mean_reproj_dist,
    })
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
