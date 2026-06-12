// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Live preview of one camera, streamed through Braid's `cam-proxy`.
//!
//! This connects to the camera's Strand Camera HTTP server via the mainbrain
//! reverse proxy, subscribes to its event stream (which carries the
//! "firehose" video frames including tracked points), and displays the
//! frames in a [`VideoField`]. It implements the same receiver-paced flow
//! control as the Strand Camera UI: after a frame is rendered, a
//! `FirehoseNotify` callback is posted so the server sends the next frame.

use std::{cell::RefCell, rc::Rc};

use gloo_events::EventListener;
use wasm_bindgen::{JsCast, JsValue, UnwrapThrowExt};
use web_sys::{EventSource, MessageEvent};
use yew::{Component, Context, Event, Html, Properties, html};

use ads_webasm::{components::VideoField, video_data::VideoData};
use strand_cam_storetype::{CallbackType, StoreType};
use strand_http_video_streaming_types::ToClient as FirehoseImageData;

pub(crate) struct CamPreview {
    es: EventSource,
    _listeners: Vec<EventListener>,
    conn_key: Option<String>,
    server_state: Option<Box<StoreType>>,
    video_data: Rc<RefCell<VideoData>>,
    full_window: bool,
}

pub(crate) enum Msg {
    NewConnKey(String),
    NewServerState(Box<StoreType>),
    NewImageFrame(FirehoseImageData),
    RenderedImage(strand_bui_backend_session_types::ConnectionKey),
    SetFullWindow(bool),
    EsError,
    Nop,
}

#[derive(PartialEq, Properties)]
pub(crate) struct Props {
    /// URL path prefix (including trailing slash) which reaches the Strand
    /// Camera HTTP server through the braid camera proxy, e.g.
    /// `/cam-proxy/<encoded-cam-name>/`.
    pub(crate) proxy_prefix: String,
    /// Camera name, displayed as part of the title.
    pub(crate) cam_name: String,
}

impl Component for CamPreview {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let events_url = format!(
            "{}{}",
            ctx.props().proxy_prefix,
            strand_cam_storetype::STRAND_CAM_EVENTS_URL_PATH
        );
        let es = EventSource::new(&events_url)
            .map_err(|js_value: JsValue| {
                let err: js_sys::Error = js_value.dyn_into().unwrap_throw();
                err
            })
            .unwrap_throw();

        let key_callback = ctx.link().callback(Msg::NewConnKey);
        let data_callback =
            ctx.link()
                .callback(|bufstr: String| match serde_json::from_str(&bufstr) {
                    Ok(msg) => Msg::NewServerState(msg),
                    Err(e) => {
                        log::error!("error decoding camera state: {e}");
                        Msg::Nop
                    }
                });
        let stream_callback = ctx.link().callback(|bufstr: String| {
            match serde_json::from_str::<FirehoseImageData>(&bufstr) {
                Ok(image_result) => Msg::NewImageFrame(image_result),
                Err(e) => {
                    log::error!("error decoding video frame: {e}");
                    Msg::Nop
                }
            }
        });

        let mut _listeners = Vec::new();
        _listeners.push(EventListener::new(
            &es,
            strand_cam_storetype::CONN_KEY_EVENT_NAME,
            move |event: &Event| {
                let event = event.dyn_ref::<MessageEvent>().unwrap_throw();
                let text = event.data().as_string().unwrap_throw();
                key_callback.emit(text);
            },
        ));
        _listeners.push(EventListener::new(
            &es,
            strand_cam_storetype::STRAND_CAM_EVENT_NAME,
            move |event: &Event| {
                let event = event.dyn_ref::<MessageEvent>().unwrap_throw();
                let text = event.data().as_string().unwrap_throw();
                data_callback.emit(text);
            },
        ));
        _listeners.push(EventListener::new(
            &es,
            strand_http_video_streaming_types::VIDEO_STREAM_EVENT_NAME,
            move |event: &Event| {
                let event = event.dyn_ref::<MessageEvent>().unwrap_throw();
                let text = event.data().as_string().unwrap_throw();
                stream_callback.emit(text);
            },
        ));

        let link = ctx.link().clone();
        _listeners.push(EventListener::new(&es, "error", move |_event: &Event| {
            // Trigger a UI redraw to show the connection state.
            link.send_message(Msg::EsError);
        }));

        Self {
            es,
            _listeners,
            conn_key: None,
            server_state: None,
            video_data: Rc::new(RefCell::new(VideoData::new(None))),
            full_window: false,
        }
    }

    fn destroy(&mut self, _ctx: &Context<Self>) {
        // Close the proxied connection to the camera so that strand-cam stops
        // encoding and sending frames for this viewer.
        self.es.close();
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::NewConnKey(conn_key) => {
                self.conn_key = Some(conn_key);
            }
            Msg::NewServerState(server_state) => {
                self.server_state = Some(server_state);
            }
            Msg::NewImageFrame(in_msg) => {
                *self.video_data.borrow_mut() = VideoData::new(Some(in_msg));
            }
            Msg::RenderedImage(ck) => {
                // Tell the camera that we rendered this frame and are thus
                // ready for the next one.
                let url = format!("{}callback", ctx.props().proxy_prefix);
                ctx.link().send_future(async move {
                    let msg = CallbackType::FirehoseNotify(ck);
                    let buf = serde_json::to_string(&msg).unwrap_throw();
                    if let Err(e) = post_json(&url, buf).await {
                        log::error!("failed sending firehose notification: {e:?}");
                    }
                    Msg::Nop
                });
                return false;
            }
            Msg::SetFullWindow(val) => {
                self.full_window = val;
            }
            Msg::EsError => {}
            Msg::Nop => {
                return false;
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        if let (Some(conn_key), Some(shared)) = (self.conn_key.as_ref(), self.server_state.as_ref())
        {
            let title = format!("Live view - {}", ctx.props().cam_name);
            html! {
                <VideoField
                    title={title}
                    conn_key={conn_key.clone()}
                    video_data={self.video_data.clone()}
                    image_width={shared.image_width}
                    image_height={shared.image_height}
                    measured_fps={shared.measured_fps}
                    full_window={self.full_window}
                    on_rendered={ctx.link().callback(Msg::RenderedImage)}
                    on_full_window={ctx.link().callback(Msg::SetFullWindow)}
                />
            }
        } else if self.es.ready_state() == 2 {
            // 0: connecting, 1: open, 2: closed
            html! {
                <p>{"Connection to camera closed."}</p>
            }
        } else {
            html! {
                <p>{"Connecting to camera..."}</p>
            }
        }
    }
}

async fn post_json(url: &str, buf: String) -> Result<(), crate::FetchError> {
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Request, RequestInit, Response};
    let opts = RequestInit::new();
    opts.set_method("POST");
    opts.set_cache(web_sys::RequestCache::NoStore);
    opts.set_body(&JsValue::from_str(&buf));
    let headers = web_sys::Headers::new().unwrap_throw();
    headers
        .append("Content-Type", "application/json")
        .unwrap_throw();
    opts.set_headers(&headers);

    let request = Request::new_with_str_and_init(url, &opts)?;

    let window = gloo_utils::window();
    let resp_value = JsFuture::from(window.fetch_with_request(&request)).await?;
    let resp: Response = resp_value.dyn_into().unwrap_throw();

    let _text = JsFuture::from(resp.text()?).await?;
    Ok(())
}
