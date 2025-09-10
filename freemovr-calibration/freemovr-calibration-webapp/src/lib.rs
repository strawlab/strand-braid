use ads_webasm::components::{
    obj_widget::MaybeValidObjFile, CsvDataField, MaybeCsvData, ObjWidget,
};
use serde::{Deserialize, Serialize};
use tracing::info;
use wasm_bindgen::{JsCast, UnwrapThrowExt};
use yew::{function_component, html, Component, Context, Html};
use yew_agent::{scope_ext::AgentScopeExt, worker::WorkerProvider};
use yew_tincture::components::{Button, TypedInput, TypedInputStorage};

use freemovr_calibration::types::{
    CompleteCorrespondance, SimpleDisplay, SimpleUVCorrespondance, VDispInfo,
};
use freemovr_calibration::TriMeshGeom;

pub mod agent;
use agent::MyWorker;

const EXR_COMMENT: Option<&str> = Some("Created by https://strawlab.org/vr-cal/");

#[derive(Debug, Serialize, Deserialize)]
struct MyError {}

impl From<std::num::ParseIntError> for MyError {
    fn from(_orig: std::num::ParseIntError) -> MyError {
        MyError {}
    }
}

impl From<freemovr_calibration::Error> for MyError {
    fn from(_orig: freemovr_calibration::Error) -> MyError {
        MyError {}
    }
}

impl std::fmt::Display for MyError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "err")
    }
}

pub struct Model {
    worker: yew_agent::scope_ext::WorkerBridgeHandle<MyWorker>,
    obj_file: MaybeValidObjFile,
    csv_file: MaybeCsvData<SimpleUVCorrespondance>,
    display_width: TypedInputStorage<usize>,
    display_height: TypedInputStorage<usize>,
    n_computing_exr: u8,
    n_computing_csv: u8,
    computed_exr: Option<Vec<u8>>,
    computed_csv: Option<Vec<u8>>,
    n_computing_stage_2_exr: u8,
    stage_2_csv_file: MaybeCsvData<CompleteCorrespondance>,
    computed_stage_2_exr: Option<Vec<u8>>,
}

pub enum Msg {
    ObjFile(MaybeValidObjFile),
    CsvFile(MaybeCsvData<SimpleUVCorrespondance>),

    ComputeExr,
    DownloadExr,

    ComputeCorrespondingCsv,
    DownloadCorrespondingCsv,

    CsvFile2(MaybeCsvData<CompleteCorrespondance>),
    ComputeExr2,
    DownloadExr2,

    DataReceived(agent::MyWorkerResponse),
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let cb = {
            let link = ctx.link().clone();
            move |v| link.send_message(Self::Message::DataReceived(v))
        };
        let worker = ctx.link().bridge_worker(cb.into());

        Self {
            worker,
            n_computing_exr: 0,
            n_computing_csv: 0,
            obj_file: MaybeValidObjFile::default(),
            csv_file: MaybeCsvData::Empty,
            display_width: TypedInputStorage::empty(),
            display_height: TypedInputStorage::empty(),
            computed_exr: None,
            computed_csv: None,
            n_computing_stage_2_exr: 0,
            stage_2_csv_file: MaybeCsvData::Empty,
            computed_stage_2_exr: None,
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::ObjFile(obj_file) => {
                self.computed_exr = None;
                self.computed_csv = None;
                self.obj_file = obj_file;
            }
            Msg::CsvFile(csv_file) => {
                self.computed_exr = None;
                self.computed_csv = None;
                self.csv_file = csv_file;
            }
            Msg::ComputeExr => match self.get_pinhole_cal_data() {
                Ok(src_data) => {
                    self.n_computing_exr += 1;
                    self.worker.send(agent::MyWorkerRequest::CalcExr(src_data));
                }
                Err(e) => {
                    tracing::error!("cound not get calibration data: {:?}", e);
                }
            },
            Msg::DownloadExr => {
                if let Some(ref buf) = self.computed_exr {
                    download_file(buf, "out.exr");
                }
            }
            Msg::ComputeCorrespondingCsv => match self.get_pinhole_cal_data() {
                Ok(src_data) => {
                    self.n_computing_csv += 1;
                    self.worker.send(agent::MyWorkerRequest::CalcCsv(src_data));
                }
                Err(e) => {
                    tracing::error!("cound not get calibration data: {:?}", e);
                }
            },
            Msg::DownloadCorrespondingCsv => {
                if let Some(ref buf) = self.computed_csv {
                    download_file(buf, "out.csv");
                }
            }
            Msg::CsvFile2(csv_file) => {
                info!("got csv file 2 event");
                self.computed_stage_2_exr = None;
                self.stage_2_csv_file = csv_file;
            }
            Msg::ComputeExr2 => {
                info!("compute exr 2");
                match &self.stage_2_csv_file {
                    MaybeCsvData::Valid(csv_file) => {
                        self.n_computing_stage_2_exr += 1;
                        self.worker.send(agent::MyWorkerRequest::CalcAdvancedExr(
                            csv_file.raw_buf().to_vec(),
                        ));
                    }
                    _ => {
                        tracing::error!("no CSV file loaded");
                    }
                }
            }
            Msg::DownloadExr2 => {
                if let Some(ref buf) = self.computed_stage_2_exr {
                    download_file(buf, "advanced.exr");
                }
            }
            Msg::DataReceived(from_worker) => match from_worker {
                agent::MyWorkerResponse::ExrData(d) => {
                    self.n_computing_exr -= 1;
                    match d {
                        Ok(d) => self.computed_exr = Some(d),
                        Err(e) => tracing::error!("{}", e),
                    }
                }
                agent::MyWorkerResponse::CsvData(d) => {
                    self.n_computing_csv -= 1;
                    match d {
                        Ok(d) => self.computed_csv = Some(d),
                        Err(e) => tracing::error!("{}", e),
                    }
                }
                agent::MyWorkerResponse::AdvancedExrData(d) => {
                    self.n_computing_stage_2_exr -= 1;
                    match d {
                        Ok(d) => self.computed_stage_2_exr = Some(d),
                        Err(e) => tracing::error!("{}", e),
                    }
                }
            },
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let obj_file_state = format!("{}", self.obj_file);
        let csv_file_state = format!("{}", self.csv_file);
        let stage_2_csv_file_state = format!("{}", self.stage_2_csv_file);
        let can_compute_stage_2_exr = matches!(self.stage_2_csv_file, MaybeCsvData::Valid(_));

        let missing = self.missing_for_calibration();
        let can_compute_pinhole_calibration = missing.is_empty();

        let download_exr_str = if can_compute_pinhole_calibration {
            ""
        } else {
            "Calibration not ready, cannot download."
        };

        let download_stage_2_exr_str = if self.computed_stage_2_exr.is_some() {
            "Valid CSV file loaded. EXR file computed. Ready to download."
        } else if can_compute_stage_2_exr {
            "Valid CSV file loaded. Can compute EXR file."
        } else {
            "No valid CSV file is loaded."
        };

        let n_computing =
            self.n_computing_exr + self.n_computing_csv + self.n_computing_stage_2_exr;
        let spinner_div_class = if n_computing > 0 {
            "compute-modal"
        } else {
            "display-none"
        };

        html! {
            <div>
                <div class={spinner_div_class}>
                    <div class="compute-modal-inner">
                        <p>
                            {"Performing computation."}
                        </p>
                        <div class="lds-ellipsis">

                            <div></div><div></div><div></div><div></div>

                        </div>
                    </div>
                </div>
                <h1>{"FreemoVR Pinhole Calibration Tool - Alpha Release"}</h1>
                <h3>{"by Andrew Straw, Straw Lab, University of Freiburg, Germany"}</h3>
                <p>{"This page computes a FreemoVR calibration based on a "}
                    <a href="https://en.wikipedia.org/wiki/Pinhole_camera_model">
                    {"pinhole projection model."}</a>
                </p>
                <p>{"Note: although FreemoVR supports multiple virtual displays \
                    per physical display (i.e. multiple projection paths, each \
                    using a portion of the display), this \
                    tool only supports a single virtual display per physical \
                    display. To request this feature, please contact Andrew Straw."}
                </p>

                <h2>{"Input: Display Surface Model"}</h2>
                <ObjWidget
                    button_text={"Select an OBJ file."}
                    onfile={ctx.link().callback(Msg::ObjFile)}
                    />
                <p>
                    { &obj_file_state }
                </p>

                <h2>{"Input: Display Size"}</h2>
                <p>{"Enter the size of your display in pixels (e.g. 1024 x 768)."}</p>
                <label>{"width"}
                    <TypedInput<usize>
                        storage={self.display_width.clone()}
                        />
                </label>
                <label>{"height"}
                    <TypedInput<usize>
                        storage={self.display_height.clone()}
                        />
                </label>

                <h2>{"Input: Corresponding Points"}</h2>
                <p>{"The file must be a CSV file with columns: display_x, display_y, texture_u, texture_v."}</p>
                <CsvDataField<SimpleUVCorrespondance>
                    button_text={"Select a CSV file."}
                    onfile={ctx.link().callback(Msg::CsvFile)}
                    />
                <p>
                    { &csv_file_state }
                </p>

                <h2>{"Output"}</h2>
                <div>
                    {download_exr_str}
                </div>
                <Button
                    title="Compute EXR"
                    onsignal={ctx.link().callback(|()| Msg::ComputeExr)}
                    disabled={!can_compute_pinhole_calibration}
                    />
                <Button
                    title="Download EXR"
                    onsignal={ctx.link().callback(|()| Msg::DownloadExr)}
                    disabled={self.computed_exr.is_none()}
                    />


                <div>
                    <h2>{"Advanced: Corresponding Points for Fine Tuning"}</h2>

                    <h3>{"Step 1: Compute and Download Corresponding Points for Fine Tuning"}</h3>
                    <div>
                        {"Provide all inputs above and download this CSV file. \
                        You can make adjustments to the CSV file to allow \
                        fine-tuning the calibration."}
                    </div>
                    <div>
                        {download_exr_str}
                    </div>
                    <Button
                        title="Compute Corresponding Points"
                        onsignal={ctx.link().callback(|()| Msg::ComputeCorrespondingCsv)}
                        disabled={!can_compute_pinhole_calibration}
                        />
                    <Button
                        title="Download Corresponding Points"
                        onsignal={ctx.link().callback(|()| Msg::DownloadCorrespondingCsv)}
                        disabled={self.computed_csv.is_none()}
                        />

                    <h3>{"Step 2: Edit the Corresponding Points"}</h3>
                    <p>{"With your own program, edit the CSV file you downloaded above."}</p>

                    <h3>{"Step 3: Upload the Corresponding Points"}</h3>
                        <CsvDataField<CompleteCorrespondance>
                            button_text={"Select a CSV file."}
                            onfile={ctx.link().callback(Msg::CsvFile2)}
                            />
                    <p>
                        { &stage_2_csv_file_state }
                    </p>

                    <h3>{"Step 4: Compute and download EXR file"}</h3>
                    <div>
                        {download_stage_2_exr_str}
                    </div>
                    <Button
                        title="Compute EXR"
                        onsignal={ctx.link().callback(|()| Msg::ComputeExr2)}
                        disabled={!can_compute_stage_2_exr}
                        />
                    <Button
                        title="Download EXR"
                        onsignal={ctx.link().callback(|()| Msg::DownloadExr2)}
                        disabled={self.computed_stage_2_exr.is_none()}
                        />

                </div>

            </div>
        }
    }
}

impl Model {
    fn missing_for_calibration(&self) -> Vec<&str> {
        let mut missing = vec![];
        if let MaybeValidObjFile::Valid(_obj) = &self.obj_file {
        } else {
            missing.push("display surface model .obj file");
        }
        if let MaybeCsvData::Valid(_csv) = &self.csv_file {
        } else {
            missing.push("corresponding points .csv file");
        }
        if self.display_width.parsed().is_ok() {
        } else {
            missing.push("display width");
        }
        if self.display_height.parsed().is_ok() {
        } else {
            missing.push("display height");
        }
        missing
    }

    fn get_pinhole_cal_data(&self) -> Result<freemovr_calibration::PinholeCalData, MyError> {
        let display = SimpleDisplay {
            width: self.display_width.parsed()?,
            height: self.display_height.parsed()?,
        };
        let geom = match &self.obj_file {
            MaybeValidObjFile::Valid(obj) => {
                let mesh = freemovr_calibration::as_ncollide_mesh(obj.mesh());
                TriMeshGeom::new(&mesh, Some(obj.filename.clone()))?
            }
            _ => {
                return Err(MyError {});
            }
        };
        let uv_display_points = match &self.csv_file {
            MaybeCsvData::Valid(data) => data.rows().to_vec(),
            _ => {
                return Err(MyError {});
            }
        };
        let epsilon = 1e-10;
        let data =
            freemovr_calibration::PinholeCalData::new(display, geom, uv_display_points, epsilon)?;
        Ok(data)
    }
}

fn download_file(orig_buf: &[u8], filename: &str) {
    let mime_type = "application/octet-stream";
    let b = js_sys::Uint8Array::new(&unsafe { js_sys::Uint8Array::view(orig_buf) }.into());
    let array = js_sys::Array::new();
    array.push(&b.buffer());

    let options = web_sys::BlobPropertyBag::new();
    options.set_type(mime_type);
    let blob =
        web_sys::Blob::new_with_u8_array_sequence_and_options(&array, &options).unwrap_throw();
    let data_url = web_sys::Url::create_object_url_with_blob(&blob).unwrap_throw();
    let document = web_sys::window().unwrap_throw().document().unwrap_throw();
    let anchor = document
        .create_element("a")
        .unwrap_throw()
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .unwrap_throw();

    anchor.set_href(&data_url);
    anchor.set_download(filename);
    anchor.set_target("_blank");

    anchor
        .style()
        .set_property("display", "none")
        .unwrap_throw();
    let body = document.body().unwrap_throw();
    body.append_child(&anchor).unwrap_throw();

    anchor.click();

    body.remove_child(&anchor).unwrap_throw();
    web_sys::Url::revoke_object_url(&data_url).unwrap_throw();
}

#[function_component]
pub fn App() -> Html {
    html! {
        <WorkerProvider<MyWorker> path="/worker.js">
            <Model />
        </WorkerProvider<MyWorker>>
    }
}
