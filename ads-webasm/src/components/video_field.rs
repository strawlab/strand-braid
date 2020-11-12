use bui_backend_types;
use serde::{Deserialize, Serialize};
use video_data::VideoData;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use yew::prelude::*;

use yew_tincture::components::CheckboxLabel;

use http_video_streaming_types::{CanvasDrawableShape, DrawableShape, Point, StrokeStyle};

const PLAYING_FPS: f32 = 10.0;
const PAUSED_FPS: f32 = 0.1;

#[derive(Debug)]
struct MouseCoords {
    x: f64,
    y: f64,
}

// js_serializable!(MouseCoords);
// js_deserializable!(MouseCoords);

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct ImData2 {
    pub found_points: Vec<Point>,
    pub draw_shapes: Vec<CanvasDrawableShape>,
    pub fno: u64,
    pub ts_rfc3339: String, // timestamp in RFC3339 format
    pub ck: bui_backend_types::ConnectionKey,
    pub name: Option<String>,
}

// js_serializable!(ImData2);
// js_deserializable!(ImData2);

#[derive(Debug, PartialEq, Clone)]
struct LoadedFrame {
    handle: JsValue,
    in_msg: ImData2,
}

// js_serializable!(LoadedFrame);
// js_deserializable!(LoadedFrame);

pub struct VideoField {
    show_div: bool, // synchronized to whether we are visible
    title: String,
    css_id: String,
    video_data: VideoData,
    last_frame_render_msec: f64,
    width: u32,
    height: u32,
    mouse_xy: Option<MouseCoords>,
    measured_fps: f32,
    link: ComponentLink<VideoField>,
    green_stroke: StrokeStyle,
}

pub enum Msg {
    FrameLoaded(JsValue),
    MouseMove(MouseEvent),
    ToggleCollapsed(bool),
}

#[derive(PartialEq, Clone, Properties)]
pub struct Props {
    pub title: String,
    pub video_data: VideoData,
    pub width: u32,
    pub height: u32,
    pub measured_fps: f32,
}

impl Component for VideoField {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            title: props.title,
            css_id: uuid::Uuid::new_v4().to_string(),
            video_data: props.video_data,
            last_frame_render_msec: 0.0,
            width: props.width,
            height: props.height,
            mouse_xy: None,
            measured_fps: props.measured_fps,
            show_div: true,
            link,
            green_stroke: StrokeStyle::from_rgb(0x7F, 0xFF, 0x7F),
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::MouseMove(mminfo) => {
                let client_x = mminfo.client_x() as f64;
                let client_y = mminfo.client_y() as f64;
                let document = web_sys::window().unwrap().document().unwrap();
                let canvas = document.get_element_by_id(&self.css_id).unwrap();
                let canvas: web_sys::HtmlCanvasElement = canvas
                    .dyn_into::<web_sys::HtmlCanvasElement>()
                    .map_err(|_| ())
                    .unwrap();
                let rect = canvas.get_bounding_client_rect(); // abs. size of element
                let scale_x = canvas.width() as f64 / rect.width(); // relationship bitmap vs. element for X
                let scale_y = canvas.height() as f64 / rect.height(); // relationship bitmap vs. element for Y
                let is_rotate_180 = canvas.class_list().contains("rotate-180");
                let mut x = (client_x - rect.left()) * scale_x; // scale mouse coordinates after they have
                let mut y = (client_y - rect.top()) * scale_y; // been adjusted to be relative to element
                if is_rotate_180 {
                    x = canvas.width() as f64 - x;
                    y = canvas.height() as f64 - y;
                }
                self.mouse_xy = Some(MouseCoords { x, y });
            }
            Msg::ToggleCollapsed(checked) => {
                self.show_div = checked;
            }
            Msg::FrameLoaded(handle) => {
                // Now the onload event has fired
                let rs_max_framerate = match self.show_div {
                    true => PLAYING_FPS,
                    false => PAUSED_FPS,
                };

                self.last_frame_render_msec = do_frame_loaded(
                    rs_max_framerate,
                    &self.css_id,
                    self.last_frame_render_msec,
                    handle,
                );
            }
        }
        false
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        self.title = props.title;
        self.video_data = props.video_data;
        self.width = props.width;
        self.height = props.height;
        self.measured_fps = props.measured_fps;
        if let Some(ref in_msg) = self.video_data.inner {
            let data_url = in_msg.firehose_frame_data_url.clone();
            let mut draw_shapes = in_msg.annotations.clone();
            if let Some(ref valid_display) = in_msg.valid_display {
                let line_width = 5.0;
                let green_shape =
                    DrawableShape::from_shape(valid_display, &self.green_stroke, line_width);
                draw_shapes.push(green_shape);
            }
            let in_msg2 = ImData2 {
                ck: in_msg.ck,
                fno: in_msg.fno,
                found_points: in_msg.found_points.clone(),
                name: in_msg.name.clone(),
                ts_rfc3339: in_msg.ts_rfc3339.clone(),
                draw_shapes: draw_shapes.into_iter().map(|s| s.into()).collect(),
            };
            let in_msg2 = JsValue::from_serde(&in_msg2).unwrap();

            let callback = self.link.callback(move |v| Msg::FrameLoaded(v));

            // This is typically hidden in a task. Can we make this a Task?
            let callback2 = Closure::once_into_js(move |v: JsValue| {
                callback.emit(v);
            });

            // let callback2 = Closure::once(move |v: JsValue| {
            //     callback.emit(v);
            // });

            // let callback2 = Closure::wrap(Box::new(move |v: JsValue| {
            //     callback.emit(v);
            // }) as Box<dyn FnMut(JsValue)>);

            // let img = web_sys::HtmlImageElement::new().unwrap();
            // img.set_src(&data_url);
            // img.set_onload(Some(callback2.as_ref().unchecked_ref()));

            set_frame_load_callback(data_url, in_msg2, callback2);

            // js! {
            //     @(no_return)
            //     let img = new Image();
            //     let data_url = @{data_url};
            //     let in_msg2 = @{in_msg2};
            //     let jscallback = @{callback2};
            //     img.src = data_url;
            //     img.onload = function () {
            //         let handle = {
            //             img,
            //             in_msg2,
            //         };

            //         jscallback(handle);
            //         jscallback.drop();
            //     };
            // };
        }
        true
    }

    fn view(&self) -> Html {
        html! {
            <div class="wrap-collapsible",>
              <CheckboxLabel:
                label=&self.title,
                initially_checked=self.show_div,
                oncheck=self.link.callback(|checked| Msg::ToggleCollapsed(checked)),
                />
              <div>
                <canvas width=self.width, height=self.height,
                    id=&self.css_id, class="video-field-canvas",
                    onmousemove=self.link.callback(|evt| Msg::MouseMove(evt)),
                    />
                { self.view_text() }
              </div>
            </div>
        }
    }
}

impl VideoField {
    fn view_text(&self) -> Html {
        if let Some(ref data) = self.video_data.inner {
            let mouse_str = if let Some(ref mouse_pos) = self.mouse_xy {
                format!("{}, {}", mouse_pos.x as i64, mouse_pos.y as i64)
            } else {
                "".to_string()
            };
            let fno_str = format!("{}", data.fno);
            html! {
                <div class="video-field-text",>
                    <div class="video-field-fno",>{"frame: "}{ &fno_str }</div>
                    <div class="video-field-mousepos",>{"mouse: "}{ &mouse_str }</div>
                    <div class="video-field-fps",>
                        {"frames per second: "}{ format!("{:.1}", self.measured_fps) }
                    </div>
                </div>
            }
        } else {
            html! {
                <span>{ "" }</span>
            }
        }
    }
}

#[wasm_bindgen(module = "/src/components/video_field.js")]
extern "C" {
    fn set_frame_load_callback(data_url: String, in_msg2: JsValue, jscallback: JsValue);
    fn do_frame_loaded(fps: f32, css_id: &str, last_frame_render_msec: f64, handle: JsValue)
        -> f64;
}
