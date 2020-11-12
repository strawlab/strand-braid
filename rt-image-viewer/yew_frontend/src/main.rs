#![recursion_limit = "1000"]

#[macro_use]
extern crate stdweb;

use yew::prelude::*;
use yew::services::Task;

use http_video_streaming_types::ToClient as FirehoseImageData;

use ads_webasm_ancient::components::{ReloadButton, VideoField};
use ads_webasm_ancient::services::eventsource::{EventSourceService, EventSourceTask, ReadyState};
use ads_webasm_ancient::video_data::VideoData;
use ads_webasm_ancient::EventSourceAction;

use rt_image_viewer_storetype::{StoreType, RT_IMAGE_EVENTS_URL_PATH, RT_IMAGE_EVENT_NAME};

enum Msg {
    EventSourceAction(EventSourceAction),
    NewImageFrame(FirehoseImageData),

    NewServerState(StoreType),

    FailedDecode(String),
    UpdateConnectionState(ReadyState),
}

struct Model {
    task1: Option<EventSourceTask>,
    video_data: VideoData,

    server_state: Option<StoreType>,
    fail_msg: String,
    ready_state: ReadyState,
    es: EventSourceService,
    link: ComponentLink<Self>,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_: Self::Properties, link: ComponentLink<Self>) -> Self {
        let mut result = Self {
            task1: None,
            video_data: VideoData::default(),

            server_state: None,
            fail_msg: "".to_string(),
            ready_state: ReadyState::Connecting,
            es: EventSourceService::new(),
            link,
        };
        // trigger connection on creation
        let msg = Msg::EventSourceAction(EventSourceAction::Connect);
        result.update(msg);
        result
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::EventSourceAction(action) => match action {
                EventSourceAction::Connect => {
                    let data_callback = self.link.send_back(|data_str: String| {
                        let data = serde_json::from_str(&data_str);
                        match data {
                            Ok(data_result) => Msg::NewServerState(data_result),
                            Err(e) => {
                                let err_str = format!("error decoding in data_callback: {:?}", e);
                                let e2 = err_str.clone();
                                js! { @(no_return) console.error(@{e2});}
                                Msg::FailedDecode(err_str)
                            }
                        }
                    });
                    let stream_callback = self.link.send_back(|data_str: String| {
                        let data = serde_json::from_str(&data_str);
                        match data {
                            Ok(data_result) => Msg::NewImageFrame(data_result),
                            Err(e) => {
                                let err_str = format!("error decoding in stream_callback: {:?}", e);
                                let e2 = err_str.clone();
                                js! { @(no_return) console.error(@{e2});}
                                Msg::FailedDecode(err_str)
                            }
                        }
                    });
                    let notification = self.link.send_back(|status: ReadyState| match status {
                        ReadyState::Connecting => Msg::UpdateConnectionState(status),
                        ReadyState::Open => Msg::UpdateConnectionState(status),
                        ReadyState::Closed => {
                            Msg::EventSourceAction(EventSourceAction::Lost(status))
                        }
                    });
                    let mut task = self.es.connect(RT_IMAGE_EVENTS_URL_PATH, notification);
                    task.add_callback(RT_IMAGE_EVENT_NAME, data_callback)
                        .unwrap();
                    task.add_callback(
                        http_video_streaming_types::VIDEO_STREAM_EVENT_NAME,
                        stream_callback,
                    )
                    .unwrap();
                    self.task1 = Some(task);
                }
                EventSourceAction::Disconnect => {
                    self.task1.take().unwrap().cancel();
                }
                EventSourceAction::Lost(status) => {
                    self.ready_state = status;
                    self.task1 = None;
                }
            },
            Msg::UpdateConnectionState(status) => {
                self.ready_state = status;
            }
            Msg::NewImageFrame(in_msg) => {
                self.video_data = VideoData {
                    inner: Some(in_msg),
                };
            }
            Msg::NewServerState(response) => {
                self.server_state = Some(response);
            }
            Msg::FailedDecode(s) => {
                self.fail_msg = s;
            }
        }
        true
    }
}

impl Renderable<Model> for Model {
    fn view(&self) -> Html<Self> {
        html! {
            <div>
                { self.disconnected_dialog() }
                <div class="wrapper",>
                    { self.view_video() }
                </div>
            </div>
        }
    }
}

impl Model {
    fn view_video(&self) -> Html<Model> {
        if let Some(ref shared) = self.server_state {
            let (image_width, image_height, measured_fps) =
                if let Some(ref im_data) = shared.image_info {
                    (
                        im_data.image_width,
                        im_data.image_height,
                        im_data.measured_fps,
                    )
                } else {
                    // XXX FIXME TODO these values are completely made up and should be gathered from
                    // the actual images being shown
                    (1280, 1024, 9999.99)
                };

            html! {
                <VideoField: title="Live view",
                    video_data=&self.video_data,
                    width=image_width,
                    height=image_height,
                    measured_fps=measured_fps,
                />
            }
        } else {
            html! {
               <div>
                 { "" }
               </div>
            }
        }
    }

    fn disconnected_dialog(&self) -> Html<Model> {
        if self.ready_state == ReadyState::Open {
            html! {
               <div>
                 { "" }
               </div>
            }
        } else {
            html! {
                <div class="modal-container",>
                    <h1> { "Web browser not connected to rt-image-viewer" } </h1>
                    <p>{ format!("Connection State: {:?}", self.ready_state) }</p>
                    <p>{ "Please restart rt-image-viewer and " }<ReloadButton: /></p>
                </div>
            }
        }
    }
}

fn main() {
    yew::initialize();
    let app: App<Model> = App::new();
    app.mount_to_body();

    // // load CSS dynamically based on our version
    // let css_url = format!("https://strawlab.org/{}/{}/style.css",
    //     env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    // js!{
    //     @(no_return)
    //     let css_url = @{css_url};
    //     let fileref = document.createElement("link");
    //     fileref.rel = "stylesheet";
    //     fileref.type = "text/css";
    //     fileref.href = css_url;
    //     document.getElementsByTagName("head")[0].appendChild(fileref);
    // }

    yew::run_loop();
}
