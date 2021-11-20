#![recursion_limit = "1024"]

extern crate yew;

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;

use log::info;
use yew::prelude::*;

use ads_webasm::components::{CsvDataField, MaybeCsvData, ObjWidget};
use yew_tincture::components::{Button, TypedInput, TypedInputStorage};

use ads_webasm::components::obj_widget::MaybeValidObjFile;
use freemovr_calibration::types::{
    CompleteCorrespondance, SimpleDisplay, SimpleUVCorrespondance, VDispInfo,
};
use freemovr_calibration::TriMeshGeom;

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
    link: ComponentLink<Self>,
    worker: Box<dyn Bridge<MyWorker>>,
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

    DataReceived(MyWorkerResponse),
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_props: Self::Properties, link: ComponentLink<Self>) -> Self {
        let callback = link.callback(|v| Msg::DataReceived(v));
        let worker = MyWorker::bridge(callback);

        Self {
            link,
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

    fn change(&mut self, _props: ()) -> ShouldRender {
        false
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
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
                    self.worker.send(MyWorkerRequest::CalcExr(src_data));
                }
                Err(e) => {
                    log::error!("cound not get calibration data: {:?}", e);
                }
            },
            Msg::DownloadExr => {
                if let Some(ref buf) = self.computed_exr {
                    download_file(&buf, "out.exr");
                }
            }
            Msg::ComputeCorrespondingCsv => match self.get_pinhole_cal_data() {
                Ok(src_data) => {
                    self.n_computing_csv += 1;
                    self.worker.send(MyWorkerRequest::CalcCsv(src_data));
                }
                Err(e) => {
                    log::error!("cound not get calibration data: {:?}", e);
                }
            },
            Msg::DownloadCorrespondingCsv => {
                if let Some(ref buf) = self.computed_csv {
                    download_file(&buf, "out.csv");
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
                        self.worker.send(MyWorkerRequest::CalcAdvancedExr(
                            csv_file.raw_buf().to_vec(),
                        ));
                    }
                    _ => {
                        log::error!("no CSV file loaded");
                    }
                }
            }
            Msg::DownloadExr2 => {
                if let Some(ref buf) = self.computed_stage_2_exr {
                    download_file(&buf, "advanced.exr");
                }
            }
            Msg::DataReceived(from_worker) => match from_worker {
                MyWorkerResponse::ExrData(d) => {
                    self.n_computing_exr -= 1;
                    match d {
                        Ok(d) => self.computed_exr = Some(d),
                        Err(e) => log::error!("{}", e),
                    }
                }
                MyWorkerResponse::CsvData(d) => {
                    self.n_computing_csv -= 1;
                    match d {
                        Ok(d) => self.computed_csv = Some(d),
                        Err(e) => log::error!("{}", e),
                    }
                }
                MyWorkerResponse::AdvancedExrData(d) => {
                    self.n_computing_stage_2_exr -= 1;
                    match d {
                        Ok(d) => self.computed_stage_2_exr = Some(d),
                        Err(e) => log::error!("{}", e),
                    }
                }
            },
        }
        true
    }

    fn view(&self) -> Html {
        let obj_file_state = format!("{}", self.obj_file);
        let csv_file_state = format!("{}", self.csv_file);
        let stage_2_csv_file_state = format!("{}", self.stage_2_csv_file);
        let can_compute_stage_2_exr = match self.stage_2_csv_file {
            MaybeCsvData::Valid(_) => true,
            _ => false,
        };

        let missing = self.missing_for_calibration();
        let can_compute_pinhole_calibration = missing.len() == 0;

        let download_exr_str = if can_compute_pinhole_calibration {
            ""
        } else {
            "Calibration not ready, cannot download."
        };

        let download_stage_2_exr_str = if self.computed_stage_2_exr.is_some() {
            "Valid CSV file loaded. EXR file computed. Ready to download."
        } else {
            if can_compute_stage_2_exr {
                "Valid CSV file loaded. Can compute EXR file."
            } else {
                "No valid CSV file is loaded."
            }
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
                <div class=spinner_div_class>
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
                <label class=classes!("btn", "custom-file-upload")>
                    {"Select an OBJ file."}
                    <ObjWidget
                        onfile=self.link.callback(|obj_file| Msg::ObjFile(obj_file))
                        />
                </label>
                <p>
                    { &obj_file_state }
                </p>

                <h2>{"Input: Display Size"}</h2>
                <p>{"Enter the size of your display in pixels (e.g. 1024 x 768)."}</p>
                <label>{"width"}
                    <TypedInput<usize>
                        storage=self.display_width.clone()
                        />
                </label>
                <label>{"height"}
                    <TypedInput<usize>
                        storage=self.display_height.clone()
                        />
                </label>

                <h2>{"Input: Corresponding Points"}</h2>
                <p>{"The file must be a CSV file with columns: display_x, display_y, texture_u, texture_v."}</p>
                <label class=classes!("btn", "custom-file-upload")>
                    {"Select a CSV file."}
                    <CsvDataField<SimpleUVCorrespondance>
                        onfile=self.link.callback(|csv_file| Msg::CsvFile(csv_file))
                        />
                </label>
                <p>
                    { &csv_file_state }
                </p>

                <h2>{"Output"}</h2>
                <div>
                    {download_exr_str}
                </div>
                <Button
                    title="Compute EXR"
                    onsignal=self.link.callback(|()| Msg::ComputeExr)
                    disabled=!can_compute_pinhole_calibration
                    />
                <Button
                    title="Download EXR"
                    onsignal=self.link.callback(|()| Msg::DownloadExr)
                    disabled=self.computed_exr.is_none()
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
                        onsignal=self.link.callback(|()| Msg::ComputeCorrespondingCsv)
                        disabled=!can_compute_pinhole_calibration
                        />
                    <Button
                        title="Download Corresponding Points"
                        onsignal=self.link.callback(|()| Msg::DownloadCorrespondingCsv)
                        disabled=self.computed_csv.is_none()
                        />


                    <h3>{"Step 2: Edit the Corresponding Points"}</h3>
                    <p>{"With your own program, edit the CSV file you downloaded above."}</p>

                    <h3>{"Step 3: Upload the Corresponding Points"}</h3>
                    <label class=classes!("btn", "custom-file-upload")>
                        {"Select a CSV file."}
                        <CsvDataField<CompleteCorrespondance>
                            onfile=self.link.callback(|csv_file| Msg::CsvFile2(csv_file))
                            />
                    </label>
                    <p>
                        { &stage_2_csv_file_state }
                    </p>

                    <h3>{"Step 4: Compute and download EXR file"}</h3>
                    <div>
                        {download_stage_2_exr_str}
                    </div>
                    <Button
                        title="Compute EXR"
                        onsignal=self.link.callback(|()| Msg::ComputeExr2)
                        disabled=!can_compute_stage_2_exr
                        />
                    <Button
                        title="Download EXR"
                        onsignal=self.link.callback(|()| Msg::DownloadExr2)
                        disabled=self.computed_stage_2_exr.is_none()
                        />

                </div>

            </div>
        }
    }
}

impl Model {
    fn missing_for_calibration(&self) -> Vec<&str> {
        let mut missing = vec![];
        if let &MaybeValidObjFile::Valid(ref _obj) = &self.obj_file {
        } else {
            missing.push("display surface model .obj file");
        }
        if let &MaybeCsvData::Valid(ref _csv) = &self.csv_file {
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
            &MaybeValidObjFile::Valid(ref obj) => {
                TriMeshGeom::new(obj.mesh(), Some(obj.filename.clone()))?
            }
            _ => {
                return Err(MyError {});
            }
        };
        let uv_display_points = match &self.csv_file {
            &MaybeCsvData::Valid(ref data) => data.rows().to_vec(),
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
    let b = js_sys::Uint8Array::new(&unsafe { js_sys::Uint8Array::view(&orig_buf) }.into());
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
    anchor.set_download(&filename);
    anchor.set_target("_blank");

    anchor.style().set_property("display", "none").unwrap();
    let body = document.body().unwrap();
    body.append_child(&anchor).unwrap();

    anchor.click();

    body.remove_child(&anchor).unwrap();
    web_sys::Url::revoke_object_url(&data_url).unwrap();
}

pub struct MyWorker {
    link: yew::worker::AgentLink<Self>,
}

pub enum MyWorkerMsg {}

#[derive(Serialize, Deserialize, Debug)]
pub enum MyWorkerRequest {
    CalcExr(freemovr_calibration::PinholeCalData),
    CalcAdvancedExr(Vec<u8>),
    CalcCsv(freemovr_calibration::PinholeCalData),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MyWorkerResponse {
    ExrData(Result<Vec<u8>, String>),
    CsvData(Result<Vec<u8>, String>),
    AdvancedExrData(Result<Vec<u8>, String>),
}

impl yew::worker::Agent for MyWorker {
    type Reach = yew::worker::Public<Self>;

    type Message = MyWorkerMsg;
    type Input = MyWorkerRequest;
    type Output = MyWorkerResponse;

    fn create(link: yew::worker::AgentLink<Self>) -> Self {
        Self { link }
    }

    fn update(&mut self, msg: Self::Message) {
        match msg {}
    }

    fn handle_input(&mut self, msg: Self::Input, who: yew::worker::HandlerId) {
        let (save_debug_images, show_mask) = (false, false);

        match msg {
            MyWorkerRequest::CalcExr(src_data) => {
                let vdisp_data = match freemovr_calibration::compute_vdisp_images(
                    &src_data,
                    save_debug_images,
                    show_mask,
                ) {
                    Ok(mut vdisp_data) => vdisp_data.remove(0),
                    Err(e) => {
                        self.link
                            .respond(who, MyWorkerResponse::ExrData(Err(format!("{}", e))));
                        return;
                    }
                };

                let visp_info_vec: Vec<&VDispInfo> = vec![&vdisp_data];
                let float_image = match freemovr_calibration::merge_vdisp_images(
                    &visp_info_vec,
                    &src_data,
                    save_debug_images,
                    show_mask,
                ) {
                    Ok(float_image) => float_image,
                    Err(e) => {
                        self.link
                            .respond(who, MyWorkerResponse::ExrData(Err(format!("{}", e))));
                        return;
                    }
                };

                let mut exr_writer = freemovr_calibration::ExrWriter::new();
                exr_writer.update(&float_image, EXR_COMMENT);
                let exr_buf = exr_writer.buffer();

                self.link
                    .respond(who, MyWorkerResponse::ExrData(Ok(exr_buf)));
            }
            MyWorkerRequest::CalcCsv(src_data) => {
                use freemovr_calibration::PinholeCal;
                let trimesh = src_data.geom_as_trimesh().unwrap();

                let pinhole_fits = src_data.pinhole_fits();
                assert!(pinhole_fits.len() == 1);
                let (_name, cam) = &pinhole_fits[0];

                let mut csv_buf = Vec::<u8>::new();

                let jsdate = js_sys::Date::new_0();
                let iso8601_dt_str: String = jsdate.to_iso_string().into();

                let tz_offset_minutes = jsdate.get_timezone_offset();

                // get correct UTC datetime
                let created_at: Option<chrono::DateTime<chrono::Utc>> =
                    chrono::DateTime::parse_from_rfc3339(&iso8601_dt_str)
                        .ok()
                        .map(|dt| dt.with_timezone(&chrono::Utc));

                let offset = chrono::FixedOffset::west((tz_offset_minutes * 60.0) as i32);
                let created_at = created_at.map(|dt| dt.with_timezone(&offset));

                // TODO: why does chrono save this without the timezone offset information?
                match freemovr_calibration::export_to_csv(&mut csv_buf, &cam, &trimesh, created_at)
                {
                    Ok(()) => {}
                    Err(e) => {
                        self.link
                            .respond(who, MyWorkerResponse::CsvData(Err(format!("{}", e))));
                        return;
                    }
                }

                self.link
                    .respond(who, MyWorkerResponse::CsvData(Ok(csv_buf)));
            }
            MyWorkerRequest::CalcAdvancedExr(raw_buf) => {
                let save_debug_images = false;
                let mut exr_buf = Vec::<u8>::new();
                let reader = std::io::Cursor::new(raw_buf.as_slice());
                match freemovr_calibration::csv2exr(
                    reader,
                    &mut exr_buf,
                    save_debug_images,
                    EXR_COMMENT,
                ) {
                    Ok(()) => {}
                    Err(e) => {
                        self.link.respond(
                            who,
                            MyWorkerResponse::AdvancedExrData(Err(format!("{}", e))),
                        );
                        return;
                    }
                }
                self.link
                    .respond(who, MyWorkerResponse::AdvancedExrData(Ok(exr_buf)));
            }
        }
    }

    fn name_of_resource() -> &'static str {
        // Due to https://github.com/yewstack/yew/issues/2056 , this currently
        // must be the absolute path (relative to origin) of the worker.
        // Ideally, we will fix this.
        "native_worker.js"
    }
}
