use serde::{Deserialize, Serialize};

use wasm_bindgen::prelude::*;

use flydra_types::{CamHttpServerInfo, CamInfo, HttpApiCallback, HttpApiShared};
use rust_cam_bui_types::{ClockModel, RecordingPath};

use yew::format::Json;
use yew::prelude::*;
use yew::services::fetch::{Credentials, FetchOptions, FetchService, FetchTask, Request, Response};

use ads_webasm::{
    components::{Button, RecordingPathWidget, ReloadButton},
    services::eventsource::{EventSourceService, EventSourceStatus, EventSourceTask, ReadyState},
};

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
    link: ComponentLink<Self>,
    ft: Option<FetchTask>,
    shared: Option<HttpApiShared>,
    es: EventSourceTask,
    fail_msg: String,
    html_page_title: Option<String>,
    recording_path: Option<RecordingPath>,
}

// -----------------------------------------------------------------------------

// Update

enum Msg {
    /// Trigger a check of the event source state.
    EsCheckState,

    // Connected(JsValue),
    // ServerMessage(MessageEvent),
    // Error(JsValue),
    NewServerState(HttpApiShared),
    FailedDecode(String),
    DoSyncCameras,
    DoRecordCsvTables(bool),
    // Fetched(fetch::ResponseDataResult<()>),
    Ignore,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_: Self::Properties, link: ComponentLink<Self>) -> Self {
        let task = {
            let data_callback = link.callback(|Json(data)| {
                match data {
                    Ok(data_result) => Msg::NewServerState(data_result),
                    Err(e) => {
                        log::error!("{}", e);
                        Msg::FailedDecode(format!("{}", e)) //.to_string())
                    }
                }
            });
            let notification = link.callback(|status| {
                if status == EventSourceStatus::Error {
                    log::error!("event source error");
                }
                Msg::EsCheckState
            });
            let mut task = EventSourceService::new()
                .connect(flydra_types::BRAID_EVENTS_URL_PATH, notification)
                .unwrap();
            task.add_event_listener(flydra_types::BRAID_EVENT_NAME, data_callback);
            task
        };

        Self {
            link,
            ft: None,
            shared: None,
            es: task,
            fail_msg: "".to_string(),
            html_page_title: None,
            recording_path: None,
        }
    }

    fn change(&mut self, _: ()) -> ShouldRender {
        false
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::EsCheckState => {
                return true;
            }
            Msg::NewServerState(data_result) => {
                self.recording_path = data_result.csv_tables_dirname.clone();
                let title = if data_result.csv_tables_dirname.is_none() {
                    data_result.flydra_app_name.clone()
                } else {
                    format!("Saving - {}", data_result.flydra_app_name)
                };
                self.shared = Some(data_result);

                let update_title = match self.html_page_title {
                    None => true,
                    Some(ref t) => t != &title,
                };

                if update_title {
                    let doc = web_sys::window().unwrap().document().unwrap();
                    doc.set_title(&title);
                    self.html_page_title = Some(title);
                }
            }
            Msg::FailedDecode(s) => {
                self.fail_msg = s;
            }
            Msg::DoSyncCameras => {
                self.ft = self.send_message(&HttpApiCallback::DoSyncCameras);
                return false; // don't update DOM, do that on return
            }
            Msg::DoRecordCsvTables(val) => {
                self.ft = self.send_message(&HttpApiCallback::DoRecordCsvTables(val));
                return false; // don't update DOM, do that on return
            }
            Msg::Ignore => {
                return false;
            }
        }
        true
    }

    fn view(&self) -> Html {
        html! {
            <div id="page-container",>
                <div id="content-wrap",>
                    <h1 style="text-align: center;">{"Braid "}
                        <a href="https://strawlab.org/braid/",><span class="infoCircle",>{"ℹ"}</span></a>
                    </h1>
                    <img src="braid-logo-no-text.png", width="523", height="118", class="center",/>
                    {self.disconnected_dialog()}
                    {self.view_shared()}
                    <footer id="footer",>
                        {format!(
                            "Braid frontend date: {} (revision {})",
                            env!("GIT_DATE"),
                            env!("GIT_HASH")
                        )}
                    </footer>
                </div>
            </div>
        }
    }
}

// -----------------------------------------------------------------------------

// View

impl Model {
    fn send_message(&mut self, args: &HttpApiCallback) -> Option<yew::services::fetch::FetchTask> {
        let post_request = Request::post("callback")
            .header("Content-Type", "application/json;charset=UTF-8")
            .body(Json(&args))
            .expect("Failed to build request.");

        let callback =
            self.link
                .callback(move |response: Response<Json<Result<(), anyhow::Error>>>| {
                    if let (meta, Json(Ok(_body))) = response.into_parts() {
                        if meta.status.is_success() {
                            return Msg::Ignore;
                        }
                    }
                    log::error!("failed sending message");
                    Msg::Ignore
                });
        let mut options = FetchOptions::default();
        options.credentials = Some(Credentials::SameOrigin);
        match FetchService::fetch_with_options(post_request, options, callback) {
            Ok(task) => Some(task),
            Err(err) => {
                log::error!("sending message failed with error: {}", err);
                None
            }
        }
    }

    fn view_shared(&self) -> Html {
        if let Some(ref value) = self.shared {
            html! {
                <div>
                    <div>
                        <RecordingPathWidget
                            label="Record .braidz file",
                            value=self.recording_path.clone(),
                            ontoggle=self.link.callback(|checked| {Msg::DoRecordCsvTables(checked)}),
                            />
                        {view_clock_model(&value.clock_model_copy)}
                        {view_calibration(&value.calibration_filename)}
                        {view_cam_list(&value.connected_cameras)}
                        {view_model_server_link(&value.model_server_addr)}
                        <div>
                            <Button:
                                title="Synchronize Cameras",
                                onsignal=self.link.callback(|_| Msg::DoSyncCameras),
                                />
                        </div>
                    </div>
                </div>
            }
        } else {
            html! {
                <div>
                    { "" }
                </div>
            }
        }
    }

    fn disconnected_dialog(&self) -> Html {
        if self.es.ready_state() == ReadyState::Open {
            html! {
               <div>
                 { "" }
               </div>
            }
        } else {
            html! {
                <div class="modal-container",>
                    <h1> { "Web browser not connected to Braid" } </h1>
                    <p>{ format!("Connection State: {:?}", self.es.ready_state()) }</p>
                    <p>{ "Please restart Braid and " }<ReloadButton: label="reload"/></p>
                </div>
            }
        }
    }
}

fn view_clock_model(clock_model: &Option<ClockModel>) -> Html {
    if let Some(ref cm) = clock_model {
        html! {
            <div>
                <p>
                    {format!("trigger device clock model: {:?}", cm)}
                </p>
            </div>
        }
    } else {
        html! {
            <div>
                <p>
                    {"No trigger device clock model."}
                </p>
            </div>
        }
    }
}

fn view_calibration(calibration_filename: &Option<String>) -> Html {
    if let Some(ref fname) = calibration_filename {
        html! {
            <div>
                <p>
                    {format!("Calibration: {}", fname)}
                </p>
            </div>
        }
    } else {
        html! {
            <div>
                <p>
                    {"No calibration."}
                </p>
            </div>
        }
    }
}

fn view_cam_list(cams: &Vec<CamInfo>) -> Html {
    let n_cams_msg = if cams.len() == 1 {
        "1 camera:".to_string()
    } else {
        format!("{} cameras:", cams.len())
    };
    let all_rendered: Vec<Html> = cams
        .iter()
        .map(|cci| {
            let cam_url = match cci.http_camserver_info {
                CamHttpServerInfo::NoServer => "http://127.0.0.1/notexist".to_string(),
                CamHttpServerInfo::Server(ref details) => details.guess_base_url_with_token(),
            };
            let state = format!("{:?}", cci.state);
            let stats = format!("{:?}", cci.recent_stats);
            html! {
                <li>
                    <a href=cam_url>{cci.name.as_str()}</a>
                    {" "}
                    {state}
                    {" "}
                    {stats}
                </li>
            }
        })
        .collect();
    html! {
        <div>
            <div>
                {n_cams_msg}
                <ul>
                    {all_rendered}
                </ul>
            </div>
        </div>
    }
}

fn view_model_server_link(opt_addr: &Option<std::net::SocketAddr>) -> Html {
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
        html! {
            <div>
                <a href=url,>
                    {"Model server"}
                </a>
            </div>
        }
    } else {
        html! {
            <p>
               {"Data hasn't fetched yet."}
            </p>
        }
    }
}

// -----------------------------------------------------------------------------

#[wasm_bindgen(start)]
pub fn run_app() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::start_app::<Model>();
}
