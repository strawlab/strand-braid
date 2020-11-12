#[macro_use]
extern crate seed;
use seed::prelude::*;
use seed::fetch;
use futures::Future;
use serde::{Serialize, Deserialize};

use wasm_bindgen::JsCast;
use web_sys::{MessageEvent, EventSource};

use flydra_types::{HttpApiCallback, HttpApiShared, CamInfo, CamHttpServerInfo};
use rust_cam_bui_types::ClockModel;

mod components;
use components::{reload_button, RecordingPath};

// -----------------------------------------------------------------------------

const READY_STATE_CONNECTING: u16 = 0;
const READY_STATE_OPEN: u16 = 1;
const READY_STATE_CLOSED: u16 = 2;

fn ready_state_string(rs: u16) -> &'static str {
    match rs {
        READY_STATE_CONNECTING => "connecting",
        READY_STATE_OPEN => "open",
        READY_STATE_CLOSED => "closed",
        _ => "<unknown>",
    }
}

// -----------------------------------------------------------------------------

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

// -----------------------------------------------------------------------------

#[derive(Debug,Serialize,Deserialize)]
struct MyError {
}

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
    shared: Option<HttpApiShared>,
    es: EventSource,
    connection_state: u16,
    html_page_title: Option<String>,
    recording_path: RecordingPath,
}

// -----------------------------------------------------------------------------

// Update

#[derive(Clone)]
enum Msg {
    Connected(JsValue),
    ServerMessage(MessageEvent),
    Error(JsValue),
    DoSyncCameras,
    DoRecordCsvTables(bool),
    Fetched(fetch::ResponseDataResult<()>),

    Reload,
}

fn update(msg: Msg, mut model: &mut Model, orders: &mut impl Orders<Msg>) {
    match msg {
        Msg::Connected(_) => {
            model.connection_state = model.es.ready_state();
        }
        Msg::Error(_) => {
            model.connection_state = model.es.ready_state();
        }
        Msg::ServerMessage(msg_event) => {
            let txt = msg_event.data().as_string().unwrap();
            let response: Result<HttpApiShared,_> = serde_json::from_str(&txt);
            match response {
                Ok(data_result) => {
                    model.recording_path
                        .set_value(data_result.csv_tables_dirname.clone());
                    let title = if data_result.csv_tables_dirname.is_none() {
                        data_result.flydra_app_name.clone()
                    } else {
                        format!("Saving - {}", data_result.flydra_app_name)
                    };
                    model.shared = Some(data_result);

                    let update_title = match model.html_page_title {
                        None => true,
                        Some(ref t) => t!=&title,
                    };

                    if update_title {
                        let doc = web_sys::window().unwrap().document().unwrap();
                        doc.set_title(&title);
                        model.html_page_title = Some(title);
                    }

                }
                Err(e) => {
                    error!("error in response, {}", e);
                }
            };
        }
        Msg::DoSyncCameras => {
            orders
                .skip()
                .perform_cmd(send_message(&HttpApiCallback::DoSyncCameras));
        }
        Msg::DoRecordCsvTables(val) => {
            orders
                .skip()
                .perform_cmd(send_message(&HttpApiCallback::DoRecordCsvTables(val)));
        }
        Msg::Fetched(Ok(_response_data)) => {
        }
        Msg::Fetched(Err(fail_reason)) => {
            error!("callback fetch error:", fail_reason);
            orders.skip();
        }
        Msg::Reload => {
            if let Some(window) = web_sys::window() {
                if let Some(e) = window.location().reload().err() {
                    error!("error reloading: {}", e);
                }
            }
        }
    }
}

fn send_message(payload: &HttpApiCallback) -> impl Future<Output = Result<Msg,Msg>> {
    let url = "callback";
    fetch::Request::new(url)
        .method(fetch::Method::Post)
        .send_json(payload)
        .fetch_json_data(Msg::Fetched)
}

// -----------------------------------------------------------------------------

// View

fn view(model: &Model) -> Node<Msg> {
    // use wasm_bindgen::JsCast;

    div![attrs!{At::Id => "page-container"}, div![attrs!{At::Id => "content-wrap"},
        h1![attrs![At::Style => "text-align: center;"],
            "Braid ",
            a![attrs!{At::Href => "https://strawlab.org/braid/"},
            span![class!["infoCircle"],"ℹ"]],
        ],
        img![attrs!{At::Src => "braid-logo-no-text.png", At::Width => "523", At::Height => "118", At::Class => "center"}],
        disconnected_dialog(&model),
        view_shared(&model),
        footer![attrs!{At::Id => "footer"},
            format!("Braid frontend date: {} (revision {})",env!("GIT_DATE"), env!("GIT_HASH")),
        ],
    ]]
}

fn disconnected_dialog(model: &Model) -> Node<Msg> {
    if model.connection_state == READY_STATE_OPEN {
        seed::empty()
    } else {
        div![class!["modal-container"],
            h1!["Web browser not connected to Braid"],
            p![format!("Connection State: {}", ready_state_string(model.connection_state))],
            p!["Please restart Braid and ", reload_button(Msg::Reload)],
        ]
    }
}

fn view_shared(model: &Model) -> Node<Msg> {
    if let Some(ref value) = model.shared {
        div![
            div![
                model.recording_path.view_recording_path("Record .braidz file",|checked| Msg::DoRecordCsvTables(checked)),
            ],
            view_clock_model( &value.clock_model_copy ),
            view_calibration( &value.calibration_filename ),
            view_cam_list( &value.connected_cameras ),
            view_model_server_link( &value.model_server_addr ),
            div![
                button![class!["btn", "btn-inactive"],
                    "Synchronize Cameras",
                    simple_ev(Ev::Click, Msg::DoSyncCameras)
                ]
            ]
        ]
    } else {
        seed::empty()
    }
}

fn view_clock_model(clock_model: &Option<ClockModel>) -> Node<Msg> {
    if let Some(ref cm) = clock_model {
        div![
            p![format!("trigger device clock model: {:?}", cm)],
        ]
    } else {
        div![
            p!["No trigger device clock model."],
        ]
    }
}

fn view_calibration(calibration_filename: &Option<String>) -> Node<Msg> {
    if let Some(ref fname) = calibration_filename {
        p![
            format!("Calibration: {}", fname)
        ]
    } else {
        p![
            "No calibration."
        ]
    }
}

fn view_cam_list(cams: &Vec<CamInfo> ) -> Node<Msg> {
    let n_cams_msg = if cams.len() == 1 {
        "1 camera:".to_string()
    } else {
        format!("{} cameras:", cams.len())
    };
    let all_rendered: Vec<Node<Msg>> = cams.iter().map(|cci| {
        let cam_url = match cci.http_camserver_info {
            CamHttpServerInfo::NoServer => "http://127.0.0.1/notexist".to_string(),
            CamHttpServerInfo::Server(ref details) => details.guess_base_url_with_token(),
        };
        let state = format!("{:?}", cci.state );
        let stats = format!("{:?}", cci.recent_stats);
        li![
            a![attrs!{At::Href => cam_url},
                cci.name.as_str(),
            ],
            " ",
            state,
            " ",
            stats,
        ]
    }).collect();
    div![
        div![
            n_cams_msg
        ],
        ul![
            all_rendered
        ]
    ]
}

fn view_model_server_link(opt_addr: &Option<std::net::SocketAddr>) -> Node<Msg> {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    if let Some(ref addr) = opt_addr {
        let ip = if addr.ip().is_unspecified() {
            match addr.ip() {
                IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
            }
        } else {
            addr.ip().clone()
        };
        let url = format!("http://{}:{}/", ip, addr.port());
        div![
            a![attrs!{At::Href => url},
                "Model server"
            ]
        ]
    } else {
        p!["Data hasn't fetched yet."]
    }
}

// -----------------------------------------------------------------------------

fn after_mount(_: Url, orders: &mut impl Orders<Msg>) -> AfterMount<Model> {

    let recording_path = RecordingPath::new();

    // connect event source
    let events_url = flydra_types::BRAID_EVENTS_URL_PATH;
    let es = EventSource::new(events_url).unwrap();
    let connection_state = es.ready_state();
    let shared = None;

    register_es_handler("open", Msg::Connected, &es, orders);
    register_es_handler(flydra_types::BRAID_EVENT_NAME, Msg::ServerMessage, &es, orders);
    register_es_handler("error", Msg::Error, &es, orders);

    // remove loading div
    if let Some(window) = web_sys::window() {
        if let Some(doc) = window.document() {
            if let Some(loading_div) = doc.get_element_by_id("loading") {
            loading_div.remove();
            }
        }
    }

    AfterMount::new(Model {shared, es, html_page_title: None, connection_state, recording_path})
}

fn register_es_handler<T, F>(
    type_: &str,
    msg: F,
    es: &EventSource,
    orders: &mut impl Orders<Msg>,
) where
    T: wasm_bindgen::convert::FromWasmAbi + 'static,
    F: Fn(T) -> Msg + 'static,
{
    let (app, msg_mapper) = (orders.clone_app(), orders.msg_mapper());

    let closure = Closure::new(move |data| {
        app.update(msg_mapper(msg(data)));
    });

    es.add_event_listener_with_callback(type_, closure.as_ref().unchecked_ref()).unwrap();
    closure.forget();
}

#[wasm_bindgen(start)]
pub fn render() {
    seed::App::builder(update, view)
        .after_mount(after_mount)
        .build_and_start();
}
