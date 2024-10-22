use std::collections::BTreeMap;

use wasm_bindgen::{prelude::wasm_bindgen, JsCast};

use yew::prelude::*;
use yew_tincture::components::Button;

use ads_webasm::components::{CsvData, CsvDataField, MaybeCsvData};

use braid_april_cal::*;

// TODO: update webpage to allow uploading intrinsic calibration YAML files
// which are provided to `CalData::known_good_intrinsics`.

pub struct Model {
    fiducial_3d_coords: MaybeCsvData<Fiducial3DCoords>,
    per_camera_2d: BTreeMap<String, (AprilConfig, CsvData<DetectionSerializer>)>,
    computed_calibration: Option<CalibrationResult>,
}

pub enum Msg {
    Fiducial3dCoordsData(MaybeCsvData<Fiducial3DCoords>),
    DetectionSerializerData(MaybeCsvData<DetectionSerializer>),
    RemoveCamera(String),
    ComputeCal,
    DownloadXmlCal,
    DownloadPymvgCal,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            fiducial_3d_coords: MaybeCsvData::Empty,
            per_camera_2d: BTreeMap::new(),
            computed_calibration: None,
        }
    }
    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Fiducial3dCoordsData(csv_file) => {
                self.computed_calibration = None;
                self.fiducial_3d_coords = csv_file;
            }
            Msg::DetectionSerializerData(csv_file) => match csv_file {
                MaybeCsvData::Valid(csv_data) => {
                    let raw_buf: &[u8] = csv_data.raw_buf();
                    match get_apriltag_cfg(raw_buf) {
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
                            self.computed_calibration = Some(cal);
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
            Msg::DownloadXmlCal => {
                if let Some(ref cal) = self.computed_calibration {
                    let buf = cal.to_flydra_xml().unwrap();
                    download_file(&buf, "braid-calibration.xml"); // TODO: set filename to date/time?
                }
            }
            Msg::DownloadPymvgCal => {
                if let Some(ref cal) = self.computed_calibration {
                    let buf = cal.to_pymvg_json().unwrap();
                    download_file(&buf, "braid-calibration.json"); // TODO: set filename to date/time?
                }
            }
        }
        true
    }
    fn view(&self, ctx: &Context<Self>) -> Html {
        let fiducial_3d_coords_file_state = format!("{}", self.fiducial_3d_coords);

        let compute_button_title = format!(
            "Compute calibration with {} cameras",
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
               <a href="https://github.com/strawlab/strand-braid/tree/main/braid-april-cal/braid-april-cal-webapp">
               {"here"}</a>{". A related "}
               <a href="https://github.com/strawlab/dlt-april-cal/blob/main/tutorial.ipynb">
               {"tutorial"}</a>{" may also be interesting."}</p>
            <h2>{"Input: 3D coordinates of April Tag fiducial markers"}</h2>
            <p>{"The file must be a CSV file with columns: id, x, y, z."}</p>
            <label class={classes!("btn", "custom-file-upload")}>
                {"Upload a 3D coordinate CSV file."}
                <CsvDataField<Fiducial3DCoords>
                    onfile={ctx.link().callback(Msg::Fiducial3dCoordsData)}
                    />
            </label>
            <p>
                { &fiducial_3d_coords_file_state }
            </p>

            <h2>{"Input: Automatically detected camera coordinates of April Tag fiducial markers"}</h2>
            <p>{"The file must be a CSV file saved by the April Tag detector of Strand Cam. (Required \
                 columns: id, h02, h12 where (h02,h12) is tag center.)"}</p>
            <label class={classes!("btn", "custom-file-upload")}>
                {"Upload a camera coordinate CSV file."}
                <CsvDataField<DetectionSerializer>
                    onfile={ctx.link().callback(Msg::DetectionSerializerData)}
                    />
            </label>
            {self.view_camera_data(ctx)}

            <h2>{"Compute calibration"}</h2>
            <Button
                title={compute_button_title}
                onsignal={ctx.link().callback(|()| Msg::ComputeCal)}
                disabled={!self.can_compute_xml_calibration()}
                />
            {self.view_calibration_quality()}
            <h2>{"Download calibration"}</h2>
            <div>
                <p>{download_xml_str}</p>
                <p>{"An XML format is typically used in Braid (although Braid can load PyMVG JSON files).
                 PyMVG JSON files can be loaded by "}<a href="https://github.com/strawlab/pymvg">{"PyMVG"}
                 </a>{"."}</p>
            </div>
            <Button
                title="Download XML calibration"
                onsignal={ctx.link().callback(|()| Msg::DownloadXmlCal)}
                disabled={self.computed_calibration.is_none()}
                />
            <Button
                title="Download PyMVG JSON calibration"
                onsignal={ctx.link().callback(|()| Msg::DownloadPymvgCal)}
                disabled={self.computed_calibration.is_none()}
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
        let has_3d = matches!(&self.fiducial_3d_coords, MaybeCsvData::Valid(_));
        !self.per_camera_2d.is_empty() && has_3d
    }
    fn view_calibration_quality(&self) -> Html {
        if let Some(ref cal) = self.computed_calibration {
            let all_rendered = cal
                .mean_reproj_dist
                .iter()
                .map(|(cam_name, mean_reproj_dist)| {
                    html! {
                        <li>
                            {format!("Camera {}: {:.3} pixels", cam_name, mean_reproj_dist)}
                        </li>
                    }
                });

            html! {
                <div>
                    <p>{"Mean reprojection distance:"}</p>
                    <ul>
                        { for all_rendered }
                    </ul>
                </div>
            }
        } else {
            html! {
                <div></div>
            }
        }
    }
    fn view_camera_data(&self, ctx: &Context<Self>) -> Html {
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
                            onsignal={ctx.link().callback(move |()| Msg::RemoveCamera(cam_name.clone()))}
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
            known_good_intrinsics: None,
        })
    }
}

fn download_file(orig_buf: &[u8], filename: &str) {
    let mime_type = "application/octet-stream";
    let b = js_sys::Uint8Array::new(&unsafe { js_sys::Uint8Array::view(orig_buf) }.into());
    let array = js_sys::Array::new();
    array.push(&b.buffer());

    let options = web_sys::BlobPropertyBag::new();
    options.set_type(mime_type);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&array, &options).unwrap();
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

#[wasm_bindgen(start)]
pub fn run_app() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<Model>::new().render();
}
