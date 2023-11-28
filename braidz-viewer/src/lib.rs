#![recursion_limit = "1024"]

use std::collections::HashMap;

use gloo::timers::callback::Timeout;
use gloo_file::{callbacks::FileReader, File};

use wasm_bindgen::prelude::*;

use yew::prelude::*;

use plotters::{
    drawing::IntoDrawingArea,
    prelude::{ChartBuilder, Circle, FontDesc, LineSeries, GREEN, RED, WHITE},
    style::Color,
};
use plotters_canvas::CanvasBackend;

use serde::{Deserialize, Serialize};

use web_sys::{self, console::log_1, Event, HtmlInputElement};

// -----------------------------------------------------------------------------

const TOPVIEW: &str = "3d-topview-canvas";
const SIDE1VIEW: &str = "3d-side1view-canvas";

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
    timeout: Option<Timeout>,
    readers: HashMap<String, FileReader>,
    braidz_file: MaybeValidBraidzFile,
    did_error: bool,
    html_page_title: Option<String>,
    why_busy: WhyBusy,
}

pub enum Msg {
    RenderAll,
    FileChanged(File),
    Loaded(String, Vec<u8>),
    FileDropped(DragEvent),
    FileDraggedOver(DragEvent),
}

enum WhyBusy {
    NotBusy,
    LoadingFile(String),
    DrawingPlots,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            timeout: None,
            braidz_file: MaybeValidBraidzFile::default(),
            readers: HashMap::default(),
            did_error: false,
            html_page_title: None,
            why_busy: WhyBusy::NotBusy,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::RenderAll => {
                update_2d_canvas(self);
                update_canvas(self);
                self.why_busy = WhyBusy::NotBusy;
            }
            Msg::Loaded(filename, rbuf) => {
                self.why_busy = WhyBusy::DrawingPlots;
                let filesize = rbuf.len() as u64;

                let cur = zip_or_dir::ZipDirArchive::from_zip(
                    std::io::Cursor::new(rbuf),
                    filename.clone(),
                )
                .unwrap_throw();

                self.readers.remove(&filename);
                let file = match braidz_parser::braidz_parse(cur) {
                    Ok(archive) => {
                        let title = format!("{filename} - BRAIDZ Viewer");

                        let v = ValidBraidzFile {
                            filename,
                            filesize,
                            archive,
                        };

                        web_sys::window()
                            .unwrap()
                            .document()
                            .unwrap()
                            .set_title(&title);
                        self.html_page_title = Some(title);

                        MaybeValidBraidzFile::Valid(v)
                    }
                    Err(e) => {
                        let title = format!("BRAIDZ Viewer");

                        web_sys::window()
                            .unwrap()
                            .document()
                            .unwrap()
                            .set_title(&title);
                        self.html_page_title = Some(title);

                        MaybeValidBraidzFile::ParseFail(e)
                    }
                };

                self.braidz_file = file;

                // Render plots after delay (so canvas is in DOM). TODO: make
                // this more robust by triggering the render once the canvas is
                // added to the DOM.
                let handle = {
                    let link = ctx.link().clone();
                    Timeout::new(3, move || link.send_message(Msg::RenderAll))
                };

                self.timeout = Some(handle);
            }
            Msg::FileChanged(file) => {
                let filename = file.name();
                self.why_busy = WhyBusy::LoadingFile(filename.clone());
                let link = ctx.link().clone();
                let filename2 = filename.clone();
                let reader = gloo_file::callbacks::read_as_bytes(&file, move |res| {
                    link.send_message(Msg::Loaded(filename2, res.expect("failed to read file")))
                });
                self.readers.insert(filename, reader);
            }
            Msg::FileDropped(evt) => {
                evt.prevent_default();
                let files = evt.data_transfer().unwrap_throw().files();
                // log_1(&format!("files dropped: {:?}", files).into());
                if let Some(files) = files {
                    let mut result = Vec::new();
                    let files = js_sys::try_iter(&files)
                        .unwrap_throw()
                        .unwrap_throw()
                        .map(|v| web_sys::File::from(v.unwrap_throw()))
                        .map(File::from);
                    result.extend(files);
                    assert!(result.len() == 1);
                    ctx.link()
                        .send_message(Msg::FileChanged(result.pop().unwrap_throw()));
                }
            }
            Msg::FileDraggedOver(evt) => {
                evt.prevent_default();
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        use crate::MaybeValidBraidzFile::*;
        let braidz_file_part = match &self.braidz_file {
            Valid(fd) => detail_table_valid(fd),
            &NotLoaded => {
                html! {
                    <div><p></p>{"No BRAIDZ file loaded."}</div>
                }
            }
            ParseFail(_e) => {
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
        } else if let Valid(ref fd) = &self.braidz_file {
            add_2d_dom_elements(fd)
        } else {
            empty()
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

        let (spinner_div_class, spinner_msg) = match &self.why_busy {
            WhyBusy::NotBusy => ("display-none", "".to_string()),
            WhyBusy::LoadingFile(filename) => {
                ("compute-modal", format!("Loading file: \"{}\"", filename))
            }
            WhyBusy::DrawingPlots => ("compute-modal", format!("Drawing plots")),
        };

        html! {
            <div id="page-container">
                <div class={spinner_div_class}>
                    <div class="compute-modal-inner">
                        <p>
                            {spinner_msg}
                        </p>
                        <div class="lds-ellipsis">
                            <div></div><div></div><div></div><div></div>
                        </div>
                    </div>
                </div>
                <div id="content-wrap"
                    ondrop={ctx.link().callback(Msg::FileDropped)}
                    ondragover={ctx.link().callback(Msg::FileDraggedOver)}>
                    <h1>{"BRAIDZ Viewer"}</h1>
                    <p>
                        {"Viewer for files saved by "}
                        <a href="https://strawlab.org/braid">{"Braid"}</a>{". Created by the "}
                        <a href="https://strawlab.org/">{"Straw Lab"}</a>{", University of Freiburg."}
                    </p>
                    <p>
                    </p>
                    <div class={"file-upload-div"}>
                        <label class={classes!("btn","custom-file-upload")}>{"Select a BRAIDZ file."}
                            <input
                                type="file"
                                class={"custom-file-upload-input"}
                                accept={".braidz"}
                                multiple=false
                                onchange={ctx.link().callback(move |e: Event| {
                                    let mut result = Vec::new();
                                    let input: HtmlInputElement = e.target_unchecked_into();

                                    if let Some(files) = input.files() {
                                        let files = js_sys::try_iter(&files)
                                            .unwrap_throw()
                                            .unwrap_throw()
                                            .map(|v| web_sys::File::from(v.unwrap_throw()))
                                            .map(File::from);
                                        result.extend(files);
                                    }
                                    assert!(result.len()==1);
                                    Msg::FileChanged(result.pop().unwrap_throw())
                                })}
                            />
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
    if let MaybeValidBraidzFile::Valid(fd) = &model.braidz_file {
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
            root.fill(&WHITE).unwrap_throw();

            match &fd.archive.data2d_distorted {
                Some(d2d) => {
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
                        .unwrap_throw();

                    chart
                        .configure_mesh()
                        .x_labels(3)
                        .y_labels(3)
                        .x_desc("Frame")
                        .y_desc("Pixel")
                        .draw()
                        .unwrap_throw();

                    chart
                        .draw_series(
                            seq.frame
                                .iter()
                                .zip(seq.xdata.iter())
                                .map(|(frame, x)| Circle::new((*frame, **x), 2, RED.filled())),
                        )
                        .unwrap_throw();

                    chart
                        .draw_series(
                            seq.frame
                                .iter()
                                .zip(seq.ydata.iter())
                                .map(|(frame, y)| Circle::new((*frame, **y), 2, GREEN.filled())),
                        )
                        .unwrap_throw();
                }
                &None => {
                    log_1(&("no data2d_distorted - cannot plot".into()));
                }
            }
        }
    }
}

fn update_canvas(model: &mut Model) {
    let mut trajectories = None;
    let mut xlim = -1.0..1.0;
    let mut ylim = -1.0..1.0;
    let mut zlim = -1.0..1.0;
    if let MaybeValidBraidzFile::Valid(fd) = &model.braidz_file {
        if let Some(ref k) = &fd.archive.kalman_estimates_info {
            trajectories = Some(&k.trajectories);
            xlim = k.xlim[0]..k.xlim[1];
            ylim = k.ylim[0]..k.ylim[1];
            zlim = k.zlim[0]..k.zlim[1];
        }
    }

    if trajectories.is_none() {
        return;
    }

    let mut do_3d_plots = false;
    if xlim.start.is_finite()
        && xlim.end.is_finite()
        && ylim.start.is_finite()
        && ylim.end.is_finite()
        && zlim.start.is_finite()
        && zlim.end.is_finite()
    {
        do_3d_plots = true;
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

        root.fill(&WHITE).unwrap_throw();

        let mut chart = ChartBuilder::on(&root)
            // .caption(format!("y=x^{}", pow), font)
            .x_label_area_size(30)
            .y_label_area_size(30)
            .build_cartesian_2d(xlim.clone(), ylim)
            .unwrap_throw();

        chart
            .configure_mesh()
            .x_labels(3)
            .y_labels(3)
            .x_desc("x (m)")
            .y_desc("y (m)")
            .draw()
            .unwrap_throw();

        if let Some(traj) = trajectories {
            for (_obj_id, traj_data) in traj.iter() {
                chart
                    .draw_series(LineSeries::new(
                        traj_data
                            .position
                            .iter()
                            .map(|pt| (pt[0] as f64, pt[1] as f64)),
                        &RED,
                    ))
                    .unwrap_throw();
            }
        }
    }

    // side1 view
    if do_3d_plots {
        let backend = CanvasBackend::new(SIDE1VIEW).unwrap_throw();
        let root = backend.into_drawing_area();
        let _font: FontDesc = ("Arial", 20.0).into();

        root.fill(&WHITE).unwrap_throw();

        let mut chart = ChartBuilder::on(&root)
            // .caption(format!("y=x^{}", pow), font)
            .x_label_area_size(30)
            .y_label_area_size(30)
            .build_cartesian_2d(xlim, zlim)
            .unwrap_throw();

        chart
            .configure_mesh()
            .x_labels(3)
            .y_labels(3)
            .x_desc("x (m)")
            .y_desc("z (m)")
            .draw()
            .unwrap_throw();

        if let Some(traj) = trajectories {
            for (_obj_id, traj_data) in traj.iter() {
                chart
                    .draw_series(LineSeries::new(
                        traj_data
                            .position
                            .iter()
                            .map(|pt| (pt[0] as f64, pt[2] as f64)),
                        &RED,
                    ))
                    .unwrap_throw();
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
        "(No 3D data)".to_string()
    };

    let total_distance = if let Some(ref k) = &summary.kalman_estimates_summary {
        format!("{:.3}", k.total_distance)
    } else {
        "(No 3D data)".to_string()
    };

    let (bx, by, bz) = if let Some(ref k) = &summary.kalman_estimates_summary {
        (
            format!("{:.3} - {:.3}", k.x_limits[0], k.x_limits[1]),
            format!("{:.3} - {:.3}", k.y_limits[0], k.y_limits[1]),
            format!("{:.3} - {:.3}", k.z_limits[0], k.z_limits[1]),
        )
    } else {
        let n = "(No 3D data)".to_string();
        (n.clone(), n.clone(), n)
    };

    let (frame_range_str, duration_str) = match &summary.data2d_summary {
        Some(x) => (format!("{} - {}", x.frame_limits[0], x.frame_limits[1]), {
            let duration = x.time_limits[1]
                .signed_duration_since(x.time_limits[0])
                .to_std()
                .unwrap();
            let seconds = duration.as_secs_f64() % 60.0;
            let minutes = (duration.as_secs() / 60) % 60;
            let hours = (duration.as_secs() / 60) / 60;
            format!("{hours:0>2}:{minutes:0>2}:{seconds:0>2.3}")
        }),
        &None => ("no frames".to_string(), "".to_string()),
    };

    html! {
        <div>
            <table>
                <tr><td>{"File name:"}</td><td>{&fd.filename}</td></tr>
                <tr><td>{"File size:"}</td><td>{bytesize::to_string(fd.filesize, false)}</td></tr>
                <tr><td>{"Schema version:"}</td><td>{format!("{}", md.schema)}</td></tr>
                <tr><td>{"Git revision:"}</td><td>{&md.git_revision}</td></tr>
                <tr><td>{"Original recording time:"}</td><td>{orig_rec_time}</td></tr>
                <tr><td>{"Duration:"}</td><td>{duration_str}</td></tr>
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

#[wasm_bindgen(module = "/js/launch_queue_support.js")]
extern "C" {
    fn launch_queue_set_consumer(f4: &Closure<dyn FnMut(JsValue)>);
}

#[wasm_bindgen(start)]
pub fn run_app() {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
    #[allow(unused_variables)]
    let app_handle = yew::Renderer::<Model>::new().render();

    {
        // Create file handler for progressive web app (PWA) when user clicks on a
        // file in the operating system.
        let boxed = Box::new(app_handle);
        let statik: &'static mut _ = Box::leak(boxed);
        let statik2 = statik.clone();

        let on_launch_params = Closure::new(move |launch_params: JsValue| {
            let files = js_sys::Reflect::get(&launch_params, &JsValue::from_str("files")).unwrap();

            let iterator = js_sys::try_iter(&files)
                .unwrap()
                .ok_or("need to pass iterable JS values")
                .unwrap();

            for res_file_js_value in iterator {
                let file_js_value = res_file_js_value.unwrap();

                let file_future = wasm_bindgen_futures::JsFuture::from(
                    file_js_value
                        .dyn_into::<web_sys::FileSystemFileHandle>()
                        .unwrap()
                        .get_file(),
                );

                let statik3 = statik2.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let file = file_future
                        .await
                        .unwrap()
                        .dyn_into::<web_sys::File>()
                        .unwrap();
                    statik3.send_message(Msg::FileChanged(file.into()));
                })
            }
        });

        launch_queue_set_consumer(&on_launch_params);

        on_launch_params.forget();
    }
}
