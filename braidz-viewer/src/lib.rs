#![recursion_limit = "1024"]

use std::time::Duration;

use wasm_bindgen::prelude::*;

use yew::prelude::*;
use yew::services::reader::{File, FileData, ReaderTask};
use yew::services::{Task, TimeoutService};

use plotters::{
    drawing::IntoDrawingArea,
    prelude::{ChartBuilder, Circle, FontDesc, LineSeries, GREEN, RED, WHITE},
    style::Color,
};
use plotters_canvas::CanvasBackend;

use serde::{Deserialize, Serialize};

use web_sys::{self, console::log_1};

// -----------------------------------------------------------------------------

const TOPVIEW: &'static str = "3d-topview-canvas";
const SIDE1VIEW: &'static str = "3d-side1view-canvas";

// -----------------------------------------------------------------------------

pub enum MaybeValidBraidzFile {
    NotLoaded,
    ParseFail(braidz_parser::Error),
    Valid(ValidBraidzFile),
}

pub struct ValidBraidzFile {
    pub filename: String,
    filesize: u64,
    archive: braidz_parser::BraidzArchive<std::io::Cursor<Vec<u8>>>,
}

impl Default for MaybeValidBraidzFile {
    fn default() -> Self {
        MaybeValidBraidzFile::NotLoaded
    }
}

// -----------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct MyError {}

impl From<std::num::ParseIntError> for MyError {
    fn from(_orig: std::num::ParseIntError) -> MyError {
        MyError {}
    }
}

impl std::fmt::Display for MyError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "err")
    }
}

// -----------------------------------------------------------------------------

struct Model {
    link: ComponentLink<Self>,
    tasks: Vec<ReaderTask>,
    _job: Option<Box<dyn Task>>,
    braidz_file: MaybeValidBraidzFile,
    did_error: bool,
}

#[derive(Clone)]
pub enum Msg {
    // Render,
    // Render2d,
    RenderAll,
    FileChanged(File),
    // BraidzFile(MaybeValidBraidzFile),
    Loaded(FileData),
    FileDropped(DragEvent),
    FileDraggedOver(DragEvent),
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
            tasks: vec![],
            _job: None,
            braidz_file: MaybeValidBraidzFile::default(),
            did_error: false,
        }
    }

    fn change(&mut self, _: ()) -> ShouldRender {
        false
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            // Msg::Render => update_canvas(&mut model),
            // Msg::Render2d => update_2d_canvas(&mut model),
            Msg::RenderAll => {
                update_2d_canvas(self);
                update_canvas(self);
            }
            Msg::Loaded(file) => {
                let FileData { name, content } = file;
                let filename = name;
                let rbuf = content;
                let filesize = rbuf.len() as u64;

                let cur = zip_or_dir::ZipDirArchive::from_zip(
                    std::io::Cursor::new(rbuf),
                    filename.clone(),
                )
                .unwrap();

                let file = match braidz_parser::braidz_parse(cur) {
                    Ok(archive) => {
                        let v = ValidBraidzFile {
                            filename,
                            filesize,
                            archive,
                        };
                        MaybeValidBraidzFile::Valid(v)
                    }
                    Err(e) => MaybeValidBraidzFile::ParseFail(e),
                };

                self.braidz_file = file;

                // Render plots after delay (so canvas is in DOM). TODO: make
                // this more robust by triggering the render once the canvas is
                // added to the DOM.
                let handle = TimeoutService::spawn(
                    Duration::from_millis(100),
                    self.link.callback(|_| Msg::RenderAll),
                );
                self._job = Some(Box::new(handle));

                // This can be replaced by `Vec::drain_filter()` when that is stable.
                let mut i = 0;
                while i != self.tasks.len() {
                    if !self.tasks[i].is_active() {
                        let _ = self.tasks.remove(i);
                    } else {
                        i += 1;
                    }
                }
            }
            Msg::FileChanged(file) => {
                let file: File = file; // type annotation for IDE
                let task = {
                    let callback = self.link.callback(Msg::Loaded);
                    yew::services::reader::ReaderService::read_file(file, callback).unwrap()
                };
                self.tasks.push(task);
            }
            Msg::FileDropped(evt) => {
                evt.prevent_default();
                let files = evt.data_transfer().unwrap().files();
                // log_1(&format!("files dropped: {:?}", files).into());
                if let Some(files) = files {
                    let mut result = Vec::new();
                    let files = js_sys::try_iter(&files)
                        .unwrap()
                        .unwrap()
                        .into_iter()
                        .map(|v| File::from(v.unwrap()));
                    result.extend(files);
                    assert!(result.len() == 1);
                    self.link
                        .send_message(Msg::FileChanged(result.pop().unwrap()));
                }
            }
            Msg::FileDraggedOver(evt) => {
                evt.prevent_default();
            }
        }
        true
    }

    fn view(&self) -> Html {
        use crate::MaybeValidBraidzFile::*;
        let braidz_file_part = match &self.braidz_file {
            &Valid(ref fd) => detail_table_valid(&fd),
            &NotLoaded => {
                html! {
                    <div><p></p>{"No BRAIDZ file loaded."}</div>
                }
            }
            &ParseFail(ref _e) => {
                html! {
                <div>{"Parsing file failed."}</div>
                }
            }
        };

        let did_error_part = if self.did_error {
            html! {
                <div>
                    <p></p>
                    {"❌ Error: DOM element not ready prior to drawing figure. \
                      Please reload this page and try again."}
                </div>
            }
        } else {
            if let Valid(ref fd) = &self.braidz_file {
                add_2d_dom_elements(fd)
            } else {
                empty()
            }
        };

        let the_3d_part = if let Valid(ref fd) = &self.braidz_file {
            if fd.archive.kalman_estimates_info.is_some() {
                add_3d_traj_dom_elements()
            } else {
                empty()
            }
        } else {
            empty()
        };

        let spinner_div_class = if self.tasks.len() > 0 {
            "compute-modal"
        } else {
            "display-none"
        };

        html! {
            <div id="page-container">
                <div class=spinner_div_class>
                    <div class="compute-modal-inner">
                        <p>
                            {"Loading file."}
                        </p>
                        <div class="lds-ellipsis">

                            <div></div><div></div><div></div><div></div>

                        </div>
                    </div>
                </div>
                <div id="content-wrap">
                    <h1>{"BRAIDZ Viewer"}</h1>
                    <p>
                        {"Online viewer for files saved by "}
                        <a href="https://strawlab.org/braid">{"Braid"}</a>{". Created by the "}
                        <a href="https://strawlab.org/">{"Straw Lab"}</a>{", University of Freiburg."}
                    </p>
                    <p>
                    </p>
                    <div ondrop=self.link.callback(|e| Msg::FileDropped(e))
                                         ondragover=self.link.callback(|e| Msg::FileDraggedOver(e))
                                         class="file-upload-div">
                        <label class=classes!("btn","custum-file-uplad")>{"Select a BRAIDZ file."}
                            <input type="file" class="custom-file-upload-input" accept=".braidz"
                            onchange=self.link.callback(move |value| {
                                let mut result = Vec::new();
                                if let ChangeData::Files(files) = value {
                                    let files = js_sys::try_iter(&files)
                                        .unwrap()
                                        .unwrap()
                                        .into_iter()
                                        .map(|v| File::from(v.unwrap()));
                                    result.extend(files);
                                }
                                assert!(result.len()==1);
                                Msg::FileChanged(result.pop().unwrap())
                            })/>
                        </label>
                    </div>
                    <div>
                        {braidz_file_part}
                        {did_error_part}
                        {the_3d_part}
                    </div>
                    <footer id="footer">{format!("Viewer date: {} (revision {})",
                                        env!("GIT_DATE"),
                                        env!("GIT_HASH"))}
                    </footer>
                </div>
            </div>
        }
    }
}

fn empty() -> Html {
    html! {
        <></>
    }
}

fn update_2d_canvas(model: &mut Model) {
    match &model.braidz_file {
        &MaybeValidBraidzFile::Valid(ref fd) => {
            for (camid, camn) in fd.archive.cam_info.camid2camn.iter() {
                let canv_id = get_canv_id(camid);
                let backend = CanvasBackend::new(&canv_id);
                let backend = if let Some(be) = backend {
                    be
                } else {
                    model.did_error = true;
                    return;
                };
                let root = backend.into_drawing_area();
                root.fill(&WHITE).unwrap();

                match &fd.archive.data2d_distorted {
                    &Some(ref d2d) => {
                        let frame_lim = &d2d.frame_lim;
                        let seq = d2d.qz.get(camn).expect("get camn");

                        let mut chart = ChartBuilder::on(&root)
                            // .caption(format!("y=x^{}", pow), font)
                            .x_label_area_size(30)
                            .y_label_area_size(30)
                            .build_cartesian_2d(
                                frame_lim[0] as i64..frame_lim[1] as i64,
                                0.0..*seq.max_pixel,
                            )
                            .unwrap();

                        chart
                            .configure_mesh()
                            .x_labels(3)
                            .y_labels(3)
                            .x_desc("Frame")
                            .y_desc("Pixel")
                            .draw()
                            .unwrap();

                        chart
                            .draw_series(
                                seq.frame
                                    .iter()
                                    .zip(seq.xdata.iter())
                                    .map(|(frame, x)| Circle::new((*frame, **x), 2, RED.filled())),
                            )
                            .unwrap();

                        chart
                            .draw_series(
                                seq.frame.iter().zip(seq.ydata.iter()).map(|(frame, y)| {
                                    Circle::new((*frame, **y), 2, GREEN.filled())
                                }),
                            )
                            .unwrap();
                    }
                    &None => {
                        log_1(&("no data2d_distorted - cannot plot".into()));
                    }
                }
            }
        }
        _ => {}
    }
}

fn update_canvas(model: &mut Model) {
    let mut trajectories = None;
    let mut xlim = -1.0..1.0;
    let mut ylim = -1.0..1.0;
    let mut zlim = -1.0..1.0;
    match &model.braidz_file {
        &MaybeValidBraidzFile::Valid(ref fd) => {
            if let Some(ref k) = &fd.archive.kalman_estimates_info {
                trajectories = Some(&k.trajectories);
                xlim = k.xlim[0] as f64..k.xlim[1] as f64;
                ylim = k.ylim[0] as f64..k.ylim[1] as f64;
                zlim = k.zlim[0] as f64..k.zlim[1] as f64;
            }
        }
        _ => {}
    }

    if trajectories.is_none() {
        return;
    }

    let mut do_3d_plots = false;
    if xlim.start.is_finite() {
        if xlim.end.is_finite() {
            if ylim.start.is_finite() {
                if ylim.end.is_finite() {
                    if zlim.start.is_finite() {
                        if zlim.end.is_finite() {
                            do_3d_plots = true;
                        }
                    }
                }
            }
        }
    }

    // top view
    if do_3d_plots {
        let backend = if let Some(be) = CanvasBackend::new(TOPVIEW) {
            be
        } else {
            model.did_error = true;
            return;
        };
        let root = backend.into_drawing_area();
        let _font: FontDesc = ("Arial", 20.0).into();

        root.fill(&WHITE).unwrap();

        let mut chart = ChartBuilder::on(&root)
            // .caption(format!("y=x^{}", pow), font)
            .x_label_area_size(30)
            .y_label_area_size(30)
            .build_cartesian_2d(xlim.clone(), ylim)
            .unwrap();

        chart
            .configure_mesh()
            .x_labels(3)
            .y_labels(3)
            .x_desc("x (m)")
            .y_desc("y (m)")
            .draw()
            .unwrap();

        if let Some(ref traj) = trajectories {
            for (_obj_id, traj_data) in traj.iter() {
                chart
                    .draw_series(LineSeries::new(
                        traj_data
                            .position
                            .iter()
                            .map(|pt| (pt[0] as f64, pt[1] as f64)),
                        &RED,
                    ))
                    .unwrap();
            }
        }
    }

    // side1 view
    if do_3d_plots {
        let backend = CanvasBackend::new(SIDE1VIEW).unwrap();
        let root = backend.into_drawing_area();
        let _font: FontDesc = ("Arial", 20.0).into();

        root.fill(&WHITE).unwrap();

        let mut chart = ChartBuilder::on(&root)
            // .caption(format!("y=x^{}", pow), font)
            .x_label_area_size(30)
            .y_label_area_size(30)
            .build_cartesian_2d(xlim, zlim)
            .unwrap();

        chart
            .configure_mesh()
            .x_labels(3)
            .y_labels(3)
            .x_desc("x (m)")
            .y_desc("z (m)")
            .draw()
            .unwrap();

        if let Some(ref traj) = trajectories {
            for (_obj_id, traj_data) in traj.iter() {
                chart
                    .draw_series(LineSeries::new(
                        traj_data
                            .position
                            .iter()
                            .map(|pt| (pt[0] as f64, pt[2] as f64)),
                        &RED,
                    ))
                    .unwrap();
            }
        }
    }
}

fn add_2d_dom_elements(fd: &ValidBraidzFile) -> Html {
    let divs: Vec<Html> = fd
        .archive
        .cam_info
        .camid2camn
        .keys()
        .map(|camid| {
            let canv_id = get_canv_id(camid);
            html! {
                <div>
                    <p>
                        {format!("{}", camid)}
                        <canvas id={canv_id} width="1000" height="200"/>
                    </p>
                </div>
            }
        })
        .collect();
    html! {
        <div>
            {divs}
        </div>
    }
}

fn get_canv_id(camid: &str) -> String {
    format!("canv2d-{}", camid)
}

fn add_3d_traj_dom_elements() -> Html {
    html! {
        <div>
            <div>
                <p>{"Top view"}</p>
                <canvas id={TOPVIEW} width="600" height="400"/>
            </div>
            <div>
                <p>{"Side view"}</p>
                <canvas id={SIDE1VIEW} width="600" height="400"/>
            </div>
        </div>
    }
}

fn detail_table_valid(fd: &ValidBraidzFile) -> Html {
    let summary = braidz_parser::summarize_braidz(&fd.archive, fd.filename.clone(), fd.filesize); // should use this instead of recomputing all this.

    let md = &summary.metadata;

    let orig_rec_time: String = if let Some(ref ts) = summary.metadata.original_recording_time {
        let ts_msec = (ts.timestamp() as f64 * 1000.0) + (ts.timestamp_subsec_nanos() as f64 / 1e6);
        let ts_msec_js = JsValue::from_f64(ts_msec);
        let dt_js = js_sys::Date::new(&ts_msec_js);
        let dt_js_str = dt_js.to_string();
        dt_js_str.into()
    } else {
        "(Original recording time is unavailable.)".to_string()
    };

    let num_cameras = summary.cam_info.camn2camid.len();
    let num_cameras = format!("{}", num_cameras);

    let cal = match &summary.calibration_info {
        Some(ci) => match &ci.water {
            Some(n) => format!("present (water below z=0 with n={})", n),
            None => "present".to_string(),
        },
        None => "not present".to_string(),
    };

    let kest_est = if let Some(ref k) = &summary.kalman_estimates_summary {
        format!("{}", k.num_trajectories)
    } else {
        format!("(No 3D data)")
    };

    let total_distance = if let Some(ref k) = &summary.kalman_estimates_summary {
        format!("{}", k.total_distance)
    } else {
        format!("(No 3D data)")
    };

    let (bx, by, bz) = if let Some(ref k) = &summary.kalman_estimates_summary {
        (
            format!("{} {}", k.x_limits[0], k.x_limits[1]),
            format!("{} {}", k.y_limits[0], k.y_limits[1]),
            format!("{} {}", k.z_limits[0], k.z_limits[1]),
        )
    } else {
        let n = format!("(No 3D data)");
        (n.clone(), n.clone(), n)
    };

    let frame_range_str = match &summary.data2d_summary {
        &Some(ref x) => format!("{} - {}", x.frame_limits[0], x.frame_limits[1]),
        &None => "no frames".to_string(),
    };

    html! {
        <div>
            <table>
                <tr><td>{"File name:"}</td><td>{&fd.filename}</td></tr>
                <tr><td>{"File size:"}</td><td>{bytesize::to_string(fd.filesize as u64, false)}</td></tr>
                <tr><td>{"Schema version:"}</td><td>{format!("{}", md.schema)}</td></tr>
                <tr><td>{"Git revision:"}</td><td>{&md.git_revision}</td></tr>
                <tr><td>{"Original recording time:"}</td><td>{orig_rec_time}</td></tr>
                <tr><td>{"Frame range:"}</td><td>{frame_range_str}</td></tr>
                <tr><td>{"Number of cameras:"}</td><td>{num_cameras}</td></tr>
                <tr><td>{"Camera calibration:"}</td><td>{cal}</td></tr>
                <tr><td>{"Number of 3d trajectories:"}</td><td>{kest_est}</td></tr>
                <tr><td>{"Total distance:"}</td><td>{total_distance}</td></tr>
                <tr><td>{"X limits:"}</td><td>{bx}</td></tr>
                <tr><td>{"Y limits:"}</td><td>{by}</td></tr>
                <tr><td>{"Z limits:"}</td><td>{bz}</td></tr>
            </table>
        </div>
    }
}

// -----------------------------------------------------------------------------

#[wasm_bindgen(start)]
pub fn run_app() {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
    yew::start_app::<Model>();
}
