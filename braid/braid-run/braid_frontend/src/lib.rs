use std::{
    error::Error,
    fmt::{self, Debug, Display, Formatter},
};

use serde::{Deserialize, Serialize};

use gloo_events::EventListener;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Event, EventSource, MessageEvent};

use flydra_types::{CamHttpServerInfo, CamInfo, HttpApiCallback, HttpApiShared};
use rust_cam_bui_types::{ClockModel, RecordingPath};

use yew::prelude::*;
use yew_tincture::components::{Button, CheckboxLabel, TypedInput, TypedInputStorage};

use ads_webasm::components::{RecordingPathWidget, ReloadButton};

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
    shared: Option<HttpApiShared>,
    es: EventSource,
    fail_msg: String,
    html_page_title: Option<String>,
    recording_path: Option<RecordingPath>,
    fake_mkv_recording_path: Option<RecordingPath>,
    post_trigger_buffer_size_local: TypedInputStorage<usize>,
    _listener: EventListener,
}

// -----------------------------------------------------------------------------

enum Msg {
    NewServerState(HttpApiShared),
    FailedDecode(serde_json::Error),
    DoRecordCsvTables(bool),
    DoRecordMkvFiles(bool),
    SendMessageFetchState(FetchState),
    SetPostTriggerBufferSize(usize),
    PostTriggerMkvRecording,
}

// -----------------------------------------------------------------------------

pub enum FetchState {
    Fetching,
    Success,
    Failed(FetchError),
}

// -----------------------------------------------------------------------------

/// Something wrong has occurred while fetching an external resource.
#[derive(Debug, Clone, PartialEq)]
pub struct FetchError {
    err: JsValue,
}
impl Display for FetchError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(&self.err, f)
    }
}
impl Error for FetchError {}

impl From<JsValue> for FetchError {
    fn from(value: JsValue) -> Self {
        Self { err: value }
    }
}

// -----------------------------------------------------------------------------

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let es = EventSource::new(flydra_types::BRAID_EVENTS_URL_PATH)
            .map_err(|js_value: JsValue| {
                let err: js_sys::Error = js_value.dyn_into().unwrap_throw();
                err
            })
            .unwrap_throw();
        let cb = ctx
            .link()
            .callback(|bufstr: String| match serde_json::from_str(&bufstr) {
                Ok(msg) => Msg::NewServerState(msg),
                Err(e) => Msg::FailedDecode(e),
            });
        let listener =
            EventListener::new(&es, flydra_types::BRAID_EVENT_NAME, move |event: &Event| {
                let event = event.dyn_ref::<MessageEvent>().unwrap_throw();
                let text = event.data().as_string().unwrap_throw();
                cb.emit(text);
            });

        Self {
            shared: None,
            es,
            fail_msg: "".to_string(),
            html_page_title: None,
            recording_path: None,
            fake_mkv_recording_path: None,
            post_trigger_buffer_size_local: TypedInputStorage::empty(),
            _listener: listener,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::SendMessageFetchState(_fetch_state) => {
                return false;
            }
            Msg::NewServerState(data_result) => {
                self.recording_path = data_result.csv_tables_dirname.clone();
                self.fake_mkv_recording_path = data_result.fake_mkv_recording_path.clone();
                let title = if data_result.csv_tables_dirname.is_none() {
                    data_result.flydra_app_name.clone()
                } else {
                    format!("Saving - {}", data_result.flydra_app_name)
                };

                self.post_trigger_buffer_size_local
                    .set_if_not_focused(data_result.post_trigger_buffer_size);

                self.shared = Some(data_result);

                let update_title = match self.html_page_title {
                    None => true,
                    Some(ref t) => t != &title,
                };

                if update_title {
                    let doc = web_sys::window().unwrap_throw().document().unwrap_throw();
                    doc.set_title(&title);
                    self.html_page_title = Some(title);
                }
            }
            Msg::FailedDecode(err) => {
                let err: anyhow::Error = err.into();
                self.fail_msg = format!("{}", err);
            }
            Msg::DoRecordCsvTables(val) => {
                ctx.link().send_future(async move {
                    match post_callback(&HttpApiCallback::DoRecordCsvTables(val)).await {
                        Ok(()) => Msg::SendMessageFetchState(FetchState::Success),
                        Err(err) => Msg::SendMessageFetchState(FetchState::Failed(err)),
                    }
                });
                ctx.link()
                    .send_message(Msg::SendMessageFetchState(FetchState::Fetching));
                return false; // Don't update DOM, do that when backend notifies us of new state.
            }
            Msg::DoRecordMkvFiles(val) => {
                return self.send_to_all_cams(&ctx, HttpApiCallback::DoRecordMkvFiles(val));
            }
            Msg::SetPostTriggerBufferSize(val) => {
                return self.send_to_all_cams(&ctx, HttpApiCallback::SetPostTriggerBufferSize(val));
            }
            Msg::PostTriggerMkvRecording => {
                return self.send_to_all_cams(&ctx, HttpApiCallback::PostTriggerMkvRecording);
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div id="page-container">
                <div id="content-wrap">
                    <h1 style="text-align: center;">{"Braid "}
                        <a href="https://strawlab.org/braid/"><span class="infoCircle">{"ⓘ"}</span></a>
                    </h1>
                    <img src="braid-logo-no-text.png" class="center logo-img" width="523" height="118" alt="Braid logo"/>
                    {self.disconnected_dialog()}
                    {self.view_shared(ctx)}
                    <footer id="footer">
                        {format!(
                            "Braid version: {} (revision {})",
                            env!("CARGO_PKG_VERSION"),
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
    fn send_to_all_cams(&mut self, ctx: &Context<Self>, msg: HttpApiCallback) -> bool {
        ctx.link().send_future(async move {
            match post_callback(&msg).await {
                Ok(()) => Msg::SendMessageFetchState(FetchState::Success),
                Err(err) => Msg::SendMessageFetchState(FetchState::Failed(err)),
            }
        });
        ctx.link()
            .send_message(Msg::SendMessageFetchState(FetchState::Fetching));
        return false; // Don't update DOM, do that when backend notifies us of new state.
    }

    fn view_post_trigger_options(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div class="wrap-collapsible">
                <CheckboxLabel label="Post Triggering" initially_checked=true />
                <div>
                    <p>{"Acquire video into a large buffer. This enables 'going back in time' to trigger saving of images
                    that were acquired prior to the Post Trigger occurring."}</p>
                </div>
                <div>
                    <label>{"buffer size (number of frames) "}
                        <TypedInput<usize>
                            storage={self.post_trigger_buffer_size_local.clone()}
                            on_send_valid={ctx.link().callback(Msg::SetPostTriggerBufferSize)}
                            />
                    </label>

                    <Button title={"Post Trigger MKV Recording"} onsignal={ctx.link().callback(|_| Msg::PostTriggerMkvRecording)}/>
                    {"(Initiates MKV recording as set above. MKV recording must be manually stopped.)"}
                </div>
            </div>
        }
    }

    fn view_shared(&self, ctx: &Context<Self>) -> Html {
        if let Some(ref value) = self.shared {
            let record_widget = if value.all_expected_cameras_are_synced
                && value.clock_model_copy.is_some()
            {
                html! {
                    <div>
                        <div>
                            <RecordingPathWidget
                            label="Record .braidz file"
                            value={self.recording_path.clone()}
                            ontoggle={ctx.link().callback(|checked| {Msg::DoRecordCsvTables(checked)})}
                            />

                            <RecordingPathWidget
                            label="Record .mkv files"
                            value={self.fake_mkv_recording_path.clone()}
                            ontoggle={ctx.link().callback(|checked| {Msg::DoRecordMkvFiles(checked)})}
                            />
                        </div>

                        { self.view_post_trigger_options(ctx) }

                    </div>
                }
            } else {
                html! {
                    <div>{"Recording disabled until cameras are synchronized and clock model is established."}</div>
                }
            };
            let fake_sync_warning = if value.fake_sync {
                html! {
                    <div>
                        {"⚠ Emulating synchronization because no trigger box in use. Data will not be perfectly synchronized. ⚠"}
                    </div>
                }
            } else {
                html! {
                    <></>
                }
            };
            html! {
                <div>
                    {fake_sync_warning}
                    <div>
                        {record_widget}
                        {view_clock_model(&value.clock_model_copy)}
                        {view_calibration(&value.calibration_filename)}
                        {view_cam_list(&value.connected_cameras)}
                        {view_model_server_link(&value.model_server_addr)}
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
        // 0: connecting, 1: open, 2: closed
        if self.es.ready_state() != 2 {
            html! {
               <div>
                 { "" }
               </div>
            }
        } else {
            html! {
                <div class="modal-container">
                    <h1> { "Web browser not connected to Braid" } </h1>
                    <p>{ format!("Connection State: {:?}", self.es.ready_state()) }</p>
                    <p>{ "Please restart Braid and " }<ReloadButton label="reload"/></p>
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
                    <a href={cam_url}>{cci.name.as_str()}</a>
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
                <a href={url}>
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

async fn post_callback(msg: &HttpApiCallback) -> Result<(), FetchError> {
    use web_sys::{Request, RequestInit, Response};
    let mut opts = RequestInit::new();
    opts.method("POST");
    // opts.mode(web_sys::RequestMode::Cors);
    // opts.headers("Content-Type", "application/json;charset=UTF-8")
    // set SameOrigin
    let buf = serde_json::to_string(&msg).unwrap_throw();
    opts.body(Some(&JsValue::from_str(&buf)));

    let url = "callback";
    let request = Request::new_with_str_and_init(url, &opts)?;

    let window = gloo_utils::window();
    let resp_value = JsFuture::from(window.fetch_with_request(&request)).await?;
    let resp: Response = resp_value.dyn_into().unwrap_throw();

    let text = JsFuture::from(resp.text()?).await?;
    let _text_string = text.as_string().unwrap_throw();
    Ok(())
}

// -----------------------------------------------------------------------------

#[wasm_bindgen(start)]
pub fn run_app() {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
    yew::start_app::<Model>();
}
