#![recursion_limit = "1024"]

use seed::{prelude::*, *};

use plotters::{
    drawing::IntoDrawingArea,
    prelude::{ChartBuilder, Circle, FontDesc, LineSeries, GREEN, RED, WHITE},
    style::Color,
};
use plotters_canvas::CanvasBackend;

use futures::future::{Future, FutureExt};

use serde::{Deserialize, Serialize};

use gloo_file::callbacks::FileRead;
use gloo_timers::future::TimeoutFuture;

use web_sys::{self, console::log_1, Blob, File};

// -----------------------------------------------------------------------------

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

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

// Model

struct Model {
    pub val: i32,
    braidz_file: MaybeValidBraidzFile,
    reader: Option<FileRead>,
    did_error: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            val: 0,
            reader: None,
            braidz_file: MaybeValidBraidzFile::default(),
            did_error: false,
        }
    }
}

// -----------------------------------------------------------------------------

// Update

#[derive(Clone)]
pub enum Msg {
    // Render,
    // Render2d,
    RenderAll,
    FileChanged(Option<File>),
    // BraidzFile(MaybeValidBraidzFile),
    LoadedArrayBuffer((String, Result<js_sys::ArrayBuffer, String>)),
}

fn update(msg: Msg, mut model: &mut Model, orders: &mut impl Orders<Msg>) {
    match msg {
        // Msg::Render => update_canvas(&mut model),
        // Msg::Render2d => update_2d_canvas(&mut model),
        Msg::RenderAll => {
            update_2d_canvas(&mut model);
            update_canvas(&mut model);
        }
        Msg::LoadedArrayBuffer((filename, result)) => {
            match result {
                Ok(buf) => {
                    let arrbuff_value: js_sys::ArrayBuffer = buf;

                    let typebuff: js_sys::Uint8Array = js_sys::Uint8Array::new(&arrbuff_value);
                    let filesize: u64 = typebuff.length().into();

                    // ugh, why not use JS memory?
                    log_1(&("TODO: avoid copying entire array".into()));
                    let mut rbuf: Vec<u8> = vec![0; filesize as usize];
                    typebuff.copy_to(&mut rbuf);

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

                    model.braidz_file = file;

                    let fut03 = render_plots_after_delay();
                    let tryfut03 = fut03.map(|msg: Msg| Ok(msg));
                    orders.render().perform_cmd(tryfut03);
                }
                Err(_) => {
                    log_1(&("Failed reading file buf".into()));
                }
            }
        }
        Msg::FileChanged(file) => {
            if let Some(file) = file {
                let filename = file.name();
                let data: Blob = file.slice().unwrap();

                let blob = gloo_file::Blob::from_raw(data);

                let (app, msg_mapper) = (orders.clone_app(), orders.msg_mapper());

                let r = gloo_file::callbacks::read_to_array_buffer(&blob, move |buf| {
                    let buf2 = buf.map_err(|e| format!("{}", e));
                    app.update(msg_mapper(Msg::LoadedArrayBuffer((filename, buf2))));
                });

                model.reader = Some(r);
            }
        }
    }
}

fn render_plots_after_delay() -> impl Future<Output = Msg> {
    TimeoutFuture::new(3) // after 3 msec
        .map(|_: ()| Msg::RenderAll)
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
                                0.0..seq.max_pixel,
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
                                    .map(|(frame, x)| Circle::new((*frame, *x), 2, RED.filled())),
                            )
                            .unwrap();

                        chart
                            .draw_series(
                                seq.frame
                                    .iter()
                                    .zip(seq.ydata.iter())
                                    .map(|(frame, y)| Circle::new((*frame, *y), 2, GREEN.filled())),
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
            for (_obj_id, series) in traj.iter() {
                chart
                    .draw_series(LineSeries::new(
                        series.iter().map(|pt| (pt.0 as f64, pt.1 as f64)),
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
            for (_obj_id, series) in traj.iter() {
                chart
                    .draw_series(LineSeries::new(
                        series.iter().map(|pt| (pt.0 as f64, pt.2 as f64)),
                        &RED,
                    ))
                    .unwrap();
            }
        }
    }
}

// -----------------------------------------------------------------------------

// View

fn view(model: &Model) -> Node<Msg> {
    use crate::MaybeValidBraidzFile::*;
    use wasm_bindgen::JsCast;

    div![
        attrs! {At::Id => "page-container"},
        div![
            attrs! {At::Id => "content-wrap"},
            h1!["BRAIDZ Viewer"],
            p![
                "Online viewer for files saved by ",
                a![attrs! {At::Href => "https://strawlab.org/braid"}, "Braid"],
                ". Created by the ",
                a![attrs! {At::Href => "https://strawlab.org"}, "Straw Lab"],
                ", University of Freiburg."
            ],
            p![],
            div![label![
                "Select a BRAIDZ file.",
                class!["btn", "custom-file-uplad"],
                input![
                    ev(Ev::Change, |event| {
                        let file = event
                            .target()
                            .and_then(|target| target.dyn_into::<web_sys::HtmlInputElement>().ok())
                            .and_then(|file_input| file_input.files())
                            .and_then(|file_list| file_list.get(0));

                        Msg::FileChanged(file)
                    }),
                    attrs! {
                        At::Type => "file",
                        At::Class => "custom-file-upload-input",
                        At::Accept => ".braidz",
                    }
                ],
            ],],
            div![match &model.braidz_file {
                &Valid(ref fd) => detail_table_valid(&fd),
                &NotLoaded => div![p![], "No BRAIDZ file loaded."],
                &ParseFail(ref _e) => div!["Parsing file failed."],
            }],
            if model.did_error {
                div![
                    p![],
                    "❌ Error: DOM element not ready prior to drawing figure. \
                Please reload this page and try again."
                ]
            } else {
                // empty![]
                if let Valid(ref fd) = &model.braidz_file {
                    add_2d_dom_elements(fd)
                } else {
                    empty![]
                }
            },
            if let Valid(ref fd) = &model.braidz_file {
                if fd.archive.kalman_estimates_info.is_some() {
                    add_3d_traj_dom_elements()
                } else {
                    empty![]
                }
            } else {
                empty![]
            },
            footer![
                attrs! {At::Id => "footer"},
                format!(
                    "Viewer date: {} (revision {})",
                    env!("GIT_DATE"),
                    env!("GIT_HASH")
                ),
            ],
        ]
    ]
}

fn add_2d_dom_elements(fd: &ValidBraidzFile) -> Node<Msg> {
    let divs: Vec<_> = fd
        .archive
        .cam_info
        .camid2camn
        .keys()
        .map(|camid| {
            let canv_id = get_canv_id(camid);
            div![
                p![format!("{}", camid)],
                canvas![attrs! {At::Id => canv_id; At::Width => "1000"; At::Height => "200"}],
            ]
        })
        .collect();

    div![
        // div![
        //     button![ class!["btn"],
        //         simple_ev(Ev::Click, Msg::Render2d),
        //         "Render 2d data"
        //     ],
        // ],
        divs,
    ]
}

fn get_canv_id(camid: &str) -> String {
    format!("canv2d-{}", camid)
}

fn add_3d_traj_dom_elements() -> Node<Msg> {
    div![
        // div![
        //     button![ class!["btn"],
        //         simple_ev(Ev::Click, Msg::Render),
        //         "Render 3d trajectories"
        //     ],
        // ],
        div![
            p!["Top view"],
            canvas![attrs! {At::Id => TOPVIEW; At::Width => "600"; At::Height => "400"}],
        ],
        div![
            p!["Side view"],
            canvas![attrs! {At::Id => SIDE1VIEW; At::Width => "600"; At::Height => "400"}],
        ],
    ]
}

fn detail_table_valid(fd: &ValidBraidzFile) -> Node<Msg> {
    let md = &fd.archive.metadata;

    let summary = braidz_parser::summarize_braidz(&fd.archive, fd.filename.clone(), fd.filesize); // should use this instead of recomputing all this.

    let orig_rec_time: String = if let Some(ref ts) = md.original_recording_time {
        let ts_msec = (ts.timestamp() as f64 * 1000.0) + (ts.timestamp_subsec_nanos() as f64 / 1e6);
        let ts_msec_js = JsValue::from_f64(ts_msec);
        let dt_js = js_sys::Date::new(&ts_msec_js);
        let dt_js_str = dt_js.to_string();
        dt_js_str.into()
    } else {
        "(Original recording time is unavailable.)".to_string()
    };

    let num_cameras = fd.archive.cam_info.camn2camid.len();
    let num_cameras = format!("{}", num_cameras);

    let cal = match &fd.archive.calibration_info {
        Some(ci) => match &ci.water {
            Some(n) => format!("present (water below z=0 with n={})", n),
            None => "present".to_string(),
        },
        None => "not present".to_string(),
    };

    let kest_est = if let Some(ref k) = &fd.archive.kalman_estimates_info {
        format!("{}", k.trajectories.len())
    } else {
        format!("(No 3D data)")
    };

    let (bx, by, bz) = if let Some(ref k) = &fd.archive.kalman_estimates_info {
        (
            format!("{} {}", k.xlim[0], k.xlim[1]),
            format!("{} {}", k.ylim[0], k.ylim[1]),
            format!("{} {}", k.zlim[0], k.zlim[1]),
        )
    } else {
        let n = format!("(No 3D data)");
        (n.clone(), n.clone(), n)
    };

    let frame_range_str = match &fd.archive.data2d_distorted {
        &Some(ref x) => format!("{} - {}", x.frame_lim[0], x.frame_lim[1]),
        &None => "no frames".to_string(),
    };

    div![table![
        tr![td!["File name:"], td![&fd.filename],],
        tr![
            td!["File size:"],
            td![bytesize::to_string(fd.filesize as u64, false)],
        ],
        tr![td!["Schema version:"], td![format!("{}", md.schema)],],
        tr![td!["Git revision:"], td![&md.git_revision],],
        tr![td!["Original recording time:"], td![orig_rec_time],],
        tr![td!["Frame range:"], td![frame_range_str],],
        tr![td!["Number of cameras:"], td![num_cameras],],
        tr![td!["Camera calibration:"], td![cal],],
        tr![td!["Number of 3d trajectories:"], td![kest_est],],
        tr![td!["X limits:"], td![bx],],
        tr![td!["Y limits:"], td![by],],
        tr![td!["Z limits:"], td![bz],],
    ]]
}

// -----------------------------------------------------------------------------

#[wasm_bindgen(start)]
pub fn start() {
    seed::App::builder(update, view).build_and_start();
}
