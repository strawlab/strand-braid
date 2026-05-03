//! web based quick .braidz viewer
//!
//! This web app can be locally installed and views files locally (without
//! uploading them from the browser).
use std::collections::HashMap;

use gloo_file::{File, callbacks::FileReader};

use wasm_bindgen::prelude::*;

use yew::prelude::*;

use web_sys::{self, console::log_1};

use ads_webasm::components::file_input::FileInput;

// -----------------------------------------------------------------------------

const TRAJECTORY_3D_VIEW: &str = "trajectory-3d-view";
const OBJECT_TRACK_TABLE_LIMIT: usize = 50;
const TRAJECTORY_3D_LIMIT: usize = 500;

// -----------------------------------------------------------------------------

#[derive(Default)]
pub enum MaybeValidBraidzFile {
    #[default]
    NotLoaded,
    ParseFail(braidz_parser::Error),
    Valid(ValidBraidzFile),
}

pub struct ValidBraidzFile {
    pub filename: String,
    filesize: u64,
    archive: braidz_parser::BraidzArchive<std::io::Cursor<Vec<u8>>>,
}

// -----------------------------------------------------------------------------

struct Model {
    readers: HashMap<String, FileReader>,
    braidz_file: MaybeValidBraidzFile,
    render_error: Option<String>,
    html_page_title: Option<String>,
    render_after_next_paint: bool,
    why_busy: WhyBusy,
}

pub enum Msg {
    RenderAll,
    FileChanged(File),
    Loaded(String, Vec<u8>),
    Set3dView(&'static str),
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
            braidz_file: MaybeValidBraidzFile::default(),
            readers: HashMap::default(),
            render_error: None,
            html_page_title: None,
            render_after_next_paint: false,
            why_busy: WhyBusy::NotBusy,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::RenderAll => {
                self.render_error = None;
                update_2d_plots(self);
                update_3d_view(self);
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
                        let title = "BRAIDZ Viewer".to_string();

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
                self.render_error = None;
                self.render_after_next_paint =
                    matches!(self.braidz_file, MaybeValidBraidzFile::Valid(_));
            }
            Msg::Set3dView(preset) => {
                if let Err(e) = set_trajectory_view(preset) {
                    let msg = js_error_message("3D view preset failed", &e);
                    log_1(&msg.clone().into());
                    model_set_render_error(self, msg);
                }
            }
            Msg::FileChanged(file) => {
                let filename = file.name();
                self.render_error = None;
                self.why_busy = WhyBusy::LoadingFile(filename.clone());
                let link = ctx.link().clone();
                let filename2 = filename.clone();
                let reader = gloo_file::callbacks::read_as_bytes(&file, move |res| {
                    link.send_message(Msg::Loaded(filename2, res.expect("failed to read file")))
                });
                self.readers.insert(filename, reader);
            }
        }
        true
    }

    fn rendered(&mut self, ctx: &Context<Self>, _first_render: bool) {
        if self.render_after_next_paint {
            self.render_after_next_paint = false;
            ctx.link().send_message(Msg::RenderAll);
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        use crate::MaybeValidBraidzFile::*;
        let braidz_file_part = match &self.braidz_file {
            Valid(fd) => detail_table_valid(fd),
            &NotLoaded => {
                html! {
                    <section class="empty-state">
                        <h2>{"Open a .braidz file"}</h2>
                        <p>{"The archive is parsed locally in this browser."}</p>
                    </section>
                }
            }
            ParseFail(_e) => {
                html! {
                    <section class="empty-state error-state">
                        <h2>{"Parsing failed"}</h2>
                        <p>{"This file could not be read as a BRAIDZ archive."}</p>
                    </section>
                }
            }
        };

        let render_error_part = if let Some(err) = &self.render_error {
            html! {
                <section class="empty-state error-state">
                    <h2>{"Plot rendering failed"}</h2>
                    <p>{err}</p>
                </section>
            }
        } else {
            empty()
        };

        let the_2d_part = if let Valid(fd) = &self.braidz_file {
            add_2d_dom_elements(fd)
        } else {
            empty()
        };

        let the_3d_part = if let Valid(fd) = &self.braidz_file {
            if fd.archive.kalman_estimates_info.is_some() {
                add_3d_traj_dom_elements(ctx, fd)
            } else {
                empty()
            }
        } else {
            empty()
        };

        let stats_part = if let Valid(fd) = &self.braidz_file {
            stats_dom_elements(fd)
        } else {
            empty()
        };

        let (spinner_div_class, spinner_msg) = match &self.why_busy {
            WhyBusy::NotBusy => ("display-none", "".to_string()),
            WhyBusy::LoadingFile(filename) => {
                ("compute-modal", format!("Loading file: \"{}\"", filename))
            }
            WhyBusy::DrawingPlots => ("compute-modal", "Drawing plots".to_string()),
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
                <div id="content-wrap">
                    <header class="app-header">
                        <div>
                            <h1>{"BRAIDZ Viewer"}</h1>
                            <p>
                                {"Explore local "}
                                <a href="https://strawlab.org/braid">{"Braid"}</a>
                                {" archives with interactive 2D and 3D views."}
                            </p>
                        </div>
                        <FileInput
                            button_text={"Select a BRAIDZ file"}
                            accept={".braidz"}
                            multiple=false
                            on_changed={ctx.link().callback(|files: Vec<File>| {
                                assert_eq!(files.len(),1);
                                let file = files.into_iter().next().unwrap();
                                Msg::FileChanged(file)
                            })}
                        />
                    </header>
                    <main class="viewer-main">
                        {braidz_file_part}
                        {render_error_part}
                        {the_2d_part}
                        {the_3d_part}
                        {stats_part}
                    </main>
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

fn model_set_render_error(model: &mut Model, msg: String) {
    model.render_error = Some(msg);
}

fn update_2d_plots(model: &mut Model) {
    if let MaybeValidBraidzFile::Valid(fd) = &model.braidz_file {
        for (camid, camn) in fd.archive.cam_info.camid2camn.iter() {
            match &fd.archive.data2d_distorted {
                Some(d2d) => {
                    let seq = d2d.qz.get(camn).expect("get camn");
                    let plot_id = get_plot_id(camid);
                    let frames = array_from_i64(seq.frame.iter().copied());
                    let xs = array_from_f64(seq.xdata.iter().map(|x| **x));
                    let ys = array_from_f64(seq.ydata.iter().map(|y| **y));
                    if let Err(e) =
                        plot_camera_2d(&plot_id, &frames.into(), &xs.into(), &ys.into(), camid)
                    {
                        let msg = js_error_message("2D plot rendering failed", &e);
                        log_1(&msg.clone().into());
                        model.render_error = Some(msg);
                        return;
                    }
                }
                &None => {
                    log_1(&("no data2d_distorted - cannot plot".into()));
                }
            }
        }
    }
}

fn update_3d_view(model: &mut Model) {
    let MaybeValidBraidzFile::Valid(fd) = &model.braidz_file else {
        return;
    };

    let Some(k) = &fd.archive.kalman_estimates_info else {
        return;
    };

    if !k.xlim.iter().all(|v| v.is_finite())
        || !k.ylim.iter().all(|v| v.is_finite())
        || !k.zlim.iter().all(|v| v.is_finite())
    {
        return;
    }

    let trajectories = trajectories_to_js(k);
    let bounds = bounds_to_js(k.xlim, k.ylim, k.zlim);
    if let Err(e) = render_trajectories_3d(TRAJECTORY_3D_VIEW, &trajectories.into(), &bounds.into())
    {
        let msg = js_error_message("3D trajectory rendering failed", &e);
        log_1(&msg.clone().into());
        model.render_error = Some(msg);
    }
}

fn js_error_message(prefix: &str, value: &JsValue) -> String {
    if let Some(message) = value.as_string() {
        return format!("{prefix}: {message}");
    }

    if let Ok(message) = js_sys::Reflect::get(value, &"message".into())
        && let Some(message) = message.as_string()
    {
        return format!("{prefix}: {message}");
    }

    format!("{prefix}. See the browser console for details.")
}

fn add_2d_dom_elements(fd: &ValidBraidzFile) -> Html {
    let divs: Vec<Html> = fd
        .archive
        .cam_info
        .camid2camn
        .keys()
        .map(|camid| {
            let plot_id = get_plot_id(camid);
            html! {
                <article class="plot-card">
                    <div id={plot_id} class="plot plot-2d"></div>
                </article>
            }
        })
        .collect();
    html! {
        <section class="panel">
            <div class="panel-heading">
                <h2>{"Camera Detections"}</h2>
                <p>{"Frame-indexed pixel detections. Drag to pan; scroll or toolbar to zoom."}</p>
            </div>
            <div class="camera-plot-grid">{divs}</div>
        </section>
    }
}

fn get_plot_id(camid: &str) -> String {
    let safe_camid: String = camid
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("plot-2d-{}", safe_camid)
}

fn add_3d_traj_dom_elements(ctx: &Context<Model>, fd: &ValidBraidzFile) -> Html {
    let note = fd
        .archive
        .kalman_estimates_info
        .as_ref()
        .and_then(|k| {
            let omitted = k.trajectories.len().saturating_sub(TRAJECTORY_3D_LIMIT);
            (omitted > 0).then(|| {
                format!(
                    "Rendering the {} longest tracks to keep the 3D view responsive. {} shorter tracks are omitted from this view.",
                    TRAJECTORY_3D_LIMIT,
                    omitted
                )
            })
        });

    html! {
        <section class="panel">
            <div class="panel-heading">
                <h2>{"3D Trajectories"}</h2>
                <p>{format!(
                    "Blender-style navigation: middle-drag orbit, Shift+middle-drag pan, wheel zoom. Trackpad fallback: Alt+left-drag orbit, Shift+Alt+left-drag pan.",
                )}</p>
            </div>
            if let Some(note) = note {
                <p class="table-note">{note}</p>
            }
            <div class="trajectory-toolbar">
                <div class="segmented-control" aria-label="3D view presets">
                    <button type="button" data-view-preset="top-xy" onclick={ctx.link().callback(|_| Msg::Set3dView("top-xy"))}>
                        {"Top-view (XY)"}
                    </button>
                    <button type="button" data-view-preset="side-xz" onclick={ctx.link().callback(|_| Msg::Set3dView("side-xz"))}>
                        {"Side-view (XZ)"}
                    </button>
                    <button type="button" data-view-preset="free" onclick={ctx.link().callback(|_| Msg::Set3dView("free"))}>
                        {"Free view"}
                    </button>
                </div>
                <span id="trajectory-view-status" class="trajectory-view-status">
                    {"Free view, perspective"}
                </span>
            </div>
            <div id={TRAJECTORY_3D_VIEW} class="trajectory-view"></div>
        </section>
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

    let kest_est = if let Some(k) = &summary.kalman_estimates_summary {
        format!("{}", k.num_trajectories)
    } else {
        "(No 3D data)".to_string()
    };

    let total_distance = if let Some(k) = &summary.kalman_estimates_summary {
        format!("{:.3} m", k.total_distance)
    } else {
        "(No 3D data)".to_string()
    };

    let (bx, by, bz) = if let Some(k) = &summary.kalman_estimates_summary {
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
        <section class="panel">
            <div class="panel-heading">
                <h2>{"Archive Summary"}</h2>
                <p>{&fd.filename}</p>
            </div>
            <div class="metric-grid">
                {metric_card("File size", bytesize::to_string(fd.filesize, false))}
                {metric_card("Duration", duration_str)}
                {metric_card("Frame range", frame_range_str)}
                {metric_card("Cameras", num_cameras)}
                {metric_card("3D trajectories", kest_est)}
                {metric_card("Total distance", total_distance)}
            </div>
            <div class="details-grid">
                {detail_card("Recording", vec![
                    ("Schema version".to_string(), format!("{}", md.schema)),
                    ("Git revision".to_string(), md.git_revision.clone()),
                    ("Original recording time".to_string(), orig_rec_time),
                    ("Camera calibration".to_string(), cal),
                ])}
                {detail_card("Spatial bounds", vec![
                    ("X limits".to_string(), bx),
                    ("Y limits".to_string(), by),
                    ("Z limits".to_string(), bz),
                ])}
            </div>
        </section>
    }
}

fn stats_dom_elements(fd: &ValidBraidzFile) -> Html {
    html! {
        <section class="panel">
            <div class="panel-heading">
                <h2>{"Statistics"}</h2>
                <p>{"Computed from the parsed archive tables."}</p>
            </div>
            {object_stats_panel(fd)}
        </section>
    }
}

fn metric_card(label: &str, value: String) -> Html {
    html! {
        <article class="metric-card">
            <span>{label}</span>
            <strong>{value}</strong>
        </article>
    }
}

fn detail_card(title: &str, rows: Vec<(String, String)>) -> Html {
    html! {
        <article class="detail-card">
            <h3>{title}</h3>
            <dl>
                {rows.into_iter().map(|(label, value)| html! {
                    <>
                        <dt>{label}</dt>
                        <dd>{value}</dd>
                    </>
                }).collect::<Html>()}
            </dl>
        </article>
    }
}

fn object_stats_panel(fd: &ValidBraidzFile) -> Html {
    let Some(k) = &fd.archive.kalman_estimates_info else {
        return empty();
    };

    let mut tracks: Vec<_> = k.trajectories.iter().collect();
    tracks.sort_by_key(|(_obj_id, traj)| std::cmp::Reverse(traj.position.len()));
    let omitted = tracks.len().saturating_sub(OBJECT_TRACK_TABLE_LIMIT);
    let rows = tracks
        .into_iter()
        .take(OBJECT_TRACK_TABLE_LIMIT)
        .map(|(obj_id, traj)| {
            let samples = traj.position.len();
            let end_frame = traj.start_frame + samples.saturating_sub(1) as u64;
            html! {
                <tr>
                    <td>{*obj_id}</td>
                    <td>{format!("{} - {}", traj.start_frame, end_frame)}</td>
                    <td>{samples}</td>
                    <td>{format!("{:.3}", traj.distance)}</td>
                </tr>
            }
        });

    let note = if omitted > 0 {
        html! {
            <p class="table-note">
                {format!(
                    "Showing the {} longest tracks. {} shorter tracks are omitted from this table; the histogram still includes all tracks.",
                    OBJECT_TRACK_TABLE_LIMIT,
                    omitted
                )}
            </p>
        }
    } else {
        empty()
    };

    html! {
        <div class="stats-grid">
            <article class="detail-card">
                <h3>{"Object Tracks"}</h3>
                <p class="table-note">
                    {format!("Table is limited to the {} longest trajectories.", OBJECT_TRACK_TABLE_LIMIT)}
                </p>
                {note}
                <div class="table-scroll">
                    <table class="stats-table">
                        <thead>
                            <tr>
                                <th>{"Object"}</th>
                                <th>{"Frames"}</th>
                                <th>{"Samples"}</th>
                                <th>{"Distance (m)"}</th>
                            </tr>
                        </thead>
                        <tbody>{rows.collect::<Html>()}</tbody>
                    </table>
                </div>
            </article>
        </div>
    }
}

fn array_from_i64(values: impl Iterator<Item = i64>) -> js_sys::Array {
    values
        .map(|value| JsValue::from_f64(value as f64))
        .collect()
}

fn array_from_f64(values: impl Iterator<Item = f64>) -> js_sys::Array {
    values.map(JsValue::from_f64).collect()
}

fn bounds_to_js(xlim: [f64; 2], ylim: [f64; 2], zlim: [f64; 2]) -> js_sys::Object {
    let bounds = js_sys::Object::new();
    js_sys::Reflect::set(
        &bounds,
        &"x".into(),
        &array_from_f64(xlim.into_iter()).into(),
    )
    .unwrap_throw();
    js_sys::Reflect::set(
        &bounds,
        &"y".into(),
        &array_from_f64(ylim.into_iter()).into(),
    )
    .unwrap_throw();
    js_sys::Reflect::set(
        &bounds,
        &"z".into(),
        &array_from_f64(zlim.into_iter()).into(),
    )
    .unwrap_throw();
    bounds
}

fn trajectories_to_js(k: &braidz_parser::KalmanEstimatesInfo) -> js_sys::Array {
    let trajectories = js_sys::Array::new();
    let mut tracks: Vec<_> = k.trajectories.iter().collect();
    tracks.sort_by_key(|(_obj_id, traj)| std::cmp::Reverse(traj.position.len()));
    for (obj_id, traj) in tracks.into_iter().take(TRAJECTORY_3D_LIMIT) {
        let obj = js_sys::Object::new();
        let xs = array_from_f64(traj.position.iter().map(|pt| pt[0] as f64));
        let ys = array_from_f64(traj.position.iter().map(|pt| pt[1] as f64));
        let zs = array_from_f64(traj.position.iter().map(|pt| pt[2] as f64));

        js_sys::Reflect::set(&obj, &"objId".into(), &JsValue::from_f64(*obj_id as f64))
            .unwrap_throw();
        js_sys::Reflect::set(&obj, &"x".into(), &xs.into()).unwrap_throw();
        js_sys::Reflect::set(&obj, &"y".into(), &ys.into()).unwrap_throw();
        js_sys::Reflect::set(&obj, &"z".into(), &zs.into()).unwrap_throw();
        js_sys::Reflect::set(
            &obj,
            &"startFrame".into(),
            &JsValue::from_f64(traj.start_frame as f64),
        )
        .unwrap_throw();
        js_sys::Reflect::set(&obj, &"distance".into(), &JsValue::from_f64(traj.distance))
            .unwrap_throw();
        trajectories.push(&obj);
    }
    trajectories
}

// -----------------------------------------------------------------------------

#[wasm_bindgen(module = "/js/launch_queue_support.js")]
extern "C" {
    fn launch_queue_set_consumer(f4: &Closure<dyn FnMut(JsValue)>);
}

#[wasm_bindgen(module = "/js/viewer3d.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = setTrajectoryView)]
    fn set_trajectory_view(preset: &str) -> Result<(), JsValue>;
}

#[wasm_bindgen(module = "/js/plots.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = plotCamera2d)]
    fn plot_camera_2d(
        container_id: &str,
        frames: &JsValue,
        xs: &JsValue,
        ys: &JsValue,
        title: &str,
    ) -> Result<(), JsValue>;
}

#[wasm_bindgen(module = "/js/viewer3d.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = renderTrajectories3d)]
    fn render_trajectories_3d(
        container_id: &str,
        trajectories: &JsValue,
        bounds: &JsValue,
    ) -> Result<(), JsValue>;
}

pub fn main() {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
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
