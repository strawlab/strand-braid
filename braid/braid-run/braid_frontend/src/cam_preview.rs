// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Live preview tile of one camera, streamed through Braid's `cam-proxy`.
//!
//! This connects to the camera's Strand Camera HTTP server through the
//! mainbrain reverse proxy, subscribes to its event stream (which carries the
//! "firehose" video frames including detected points), and draws the frames
//! onto a compact canvas. It implements the same receiver-paced flow control
//! as the Strand Camera UI: after a frame is rendered, a `FirehoseNotify`
//! callback is posted so the server sends the next frame.

use gloo_events::EventListener;
use gloo_timers::callback::{Interval, Timeout};
use wasm_bindgen::{JsCast, JsValue, UnwrapThrowExt, closure::Closure};
use web_sys::{EventSource, MessageEvent};
use yew::{Component, Context, Event, Html, NodeRef, Properties, html};

use strand_bui_backend_session_types::ConnectionKey;
use strand_cam_storetype::CallbackType;
use strand_http_video_streaming_types::{
    CanvasDrawableShape, DrawableShape, StrokeStyle, ToClient as FirehoseImageData,
};

/// Maximum frame rate of the preview.
///
/// The preview always shows the most recent frame, so this limits resource
/// usage without adding latency.
const PREVIEW_FPS: f64 = 10.0;

/// A frame which finished loading into the `<img>` element and can be drawn.
#[derive(Clone, PartialEq)]
pub(crate) struct LoadedFrame {
    fno: u64,
    ck: ConnectionKey,
    shapes: Vec<CanvasDrawableShape>,
}

pub(crate) struct CamPreview {
    es: EventSource,
    _listeners: Vec<EventListener>,
    image: web_sys::HtmlImageElement,
    /// Keeps the current `<img>` onload closure alive. Replaced every frame.
    onload_closure: Option<Closure<dyn FnMut()>>,
    canvas_ref: NodeRef,
    green_stroke: StrokeStyle,
    rendered_fno: Option<u64>,
    /// Connection key of this event stream, as reported by the camera in the
    /// most recent video frame message. Required to send `FirehoseNotify`.
    last_ck: Option<ConnectionKey>,
    last_frame_render: f64,
    last_recv: f64,
    timeout: Option<Timeout>,
    _clock_handle: Interval,
}

pub(crate) enum Msg {
    NewImageFrame(FirehoseImageData),
    FrameLoaded(LoadedFrame),
    NotifySender,
    CheckForUpdate,
    EsError,
    Nop,
}

#[derive(PartialEq, Properties)]
pub(crate) struct Props {
    /// URL path prefix (including trailing slash) which reaches the Strand
    /// Camera HTTP server through the braid camera proxy, e.g.
    /// `/cam-proxy/<encoded-cam-name>/`.
    pub(crate) proxy_prefix: String,
    /// Inline style setting the aspect ratio of the camera image, applied to
    /// the status box shown before the first frame so that it occupies the
    /// same space as the canvas will.
    #[prop_or_default]
    pub(crate) aspect_style: Option<String>,
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

        let _clock_handle = {
            let link = ctx.link().clone();
            Interval::new(100, move || link.send_message(Msg::CheckForUpdate))
        };

        Self {
            es,
            _listeners,
            image: web_sys::HtmlImageElement::new().unwrap_throw(),
            onload_closure: None,
            canvas_ref: NodeRef::default(),
            green_stroke: StrokeStyle::from_rgb(0x7F, 0xFF, 0x7F),
            rendered_fno: None,
            last_ck: None,
            last_frame_render: 0.0,
            last_recv: 0.0,
            timeout: None,
            _clock_handle,
        }
    }

    fn destroy(&mut self, _ctx: &Context<Self>) {
        // Close the proxied connection to the camera so that strand-cam stops
        // encoding and sending frames for this viewer.
        self.es.close();
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::NewImageFrame(in_msg) => {
                self.last_recv = js_sys::Date::now();

                let mut draw_shapes = in_msg.annotations.clone();
                if let Some(ref valid_display) = in_msg.valid_display {
                    let line_width = 5.0;
                    draw_shapes.push(DrawableShape::from_shape(
                        valid_display,
                        &self.green_stroke,
                        line_width,
                    ));
                }
                let loaded = LoadedFrame {
                    fno: in_msg.fno,
                    ck: in_msg.ck,
                    shapes: draw_shapes.into_iter().map(|s| s.into()).collect(),
                };
                let callback = ctx
                    .link()
                    .callback(move |_| Msg::FrameLoaded(loaded.clone()));
                let on_load_closure = Closure::wrap(Box::new(move || {
                    callback.emit(()); // dummy arg for callback
                }) as Box<dyn FnMut()>);

                self.image.set_src(&in_msg.firehose_frame_data_url);
                self.image
                    .set_onload(Some(on_load_closure.as_ref().unchecked_ref()));
                // Keep the new closure alive; drop the previous one, which the
                // `<img>` element no longer references.
                self.onload_closure = Some(on_load_closure);
                return false;
            }
            Msg::FrameLoaded(frame) => {
                self.draw_frame_canvas(&frame);
                let first_frame = self.rendered_fno.is_none();
                self.rendered_fno = Some(frame.fno);
                self.last_ck = Some(frame.ck);

                // Wait before requesting a new frame to throttle the rate.
                let wait_msecs = {
                    let now = js_sys::Date::now(); // in milliseconds
                    let desired_dt = 1.0 / PREVIEW_FPS * 1000.0; // convert to msec
                    let desired_now = self.last_frame_render + desired_dt;
                    let wait = desired_now - now;
                    self.last_frame_render = now;
                    wait.round() as i64
                };
                if wait_msecs > 0 {
                    let link = ctx.link().clone();
                    self.timeout = Some(Timeout::new(wait_msecs as u32, move || {
                        link.send_message(Msg::NotifySender)
                    }));
                } else {
                    self.timeout = None;
                    ctx.link().send_message(Msg::NotifySender);
                }

                // The first frame switches the view from "connecting" to the
                // canvas; subsequent draws need no DOM update.
                return first_frame;
            }
            Msg::NotifySender => {
                self.timeout = None;
                if let Some(ck) = self.last_ck {
                    let url = format!("{}callback", ctx.props().proxy_prefix);
                    ctx.link().send_future(async move {
                        let msg = CallbackType::FirehoseNotify(ck);
                        let buf = serde_json::to_string(&msg).unwrap_throw();
                        if let Err(e) = post_json(&url, buf).await {
                            log::error!("failed sending firehose notification: {e:?}");
                        }
                        Msg::Nop
                    });
                }
                return false;
            }
            Msg::CheckForUpdate => {
                // If no frame arrived for too long (e.g. a notification was
                // lost), request a new one.
                let now = js_sys::Date::now(); // in milliseconds
                let dur_msec = now - self.last_recv;
                if dur_msec > (1.0 / PREVIEW_FPS * 1000.0) && self.last_ck.is_some() {
                    self.last_recv = now; // Reset timeout to limit requests.
                    ctx.link().send_message(Msg::NotifySender);
                }
                return false;
            }
            Msg::EsError => {}
            Msg::Nop => {
                return false;
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let canvas_style = if self.rendered_fno.is_some() {
            ""
        } else {
            "display: none;"
        };
        let status = if self.rendered_fno.is_some() {
            html! {}
        } else if self.es.ready_state() == 2 {
            // 0: connecting, 1: open, 2: closed
            html! {
                <div class="cam-preview-status" style={ctx.props().aspect_style.clone()}>
                    {"Connection to camera closed."}
                </div>
            }
        } else {
            html! {
                <div class="cam-preview-status" style={ctx.props().aspect_style.clone()}>
                    {"Connecting to camera..."}
                </div>
            }
        };
        html! {
            <>
                <canvas
                    ref={self.canvas_ref.clone()}
                    class="cam-preview-canvas"
                    style={canvas_style}
                    />
                {status}
            </>
        }
    }
}

impl CamPreview {
    fn draw_frame_canvas(&self, frame: &LoadedFrame) {
        let Some(canvas) = self.canvas_ref.cast::<web_sys::HtmlCanvasElement>() else {
            return;
        };
        // Match the canvas resolution to the camera image.
        let (w, h) = (self.image.natural_width(), self.image.natural_height());
        if canvas.width() != w {
            canvas.set_width(w);
        }
        if canvas.height() != h {
            canvas.set_height(h);
        }
        let ctx = web_sys::CanvasRenderingContext2d::from(JsValue::from(
            canvas.get_context("2d").unwrap_throw().unwrap_throw(),
        ));
        ctx.draw_image_with_html_image_element(&self.image, 0.0, 0.0)
            .unwrap_throw();
        ads_webasm::components::draw_shapes(&ctx, &frame.shapes);
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
