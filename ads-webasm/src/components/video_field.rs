use std::{cell::RefCell, rc::Rc};

use crate::video_data::VideoData;
use bui_backend_types;
use gloo::timers::callback::Timeout;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};
use yew::{html, Callback, Component, Context, Html, MouseEvent, Properties};

use yew_tincture::components::CheckboxLabel;

use http_video_streaming_types::{
    CanvasDrawableShape, CircleParams, FirehoseCallbackInner, Point, StrokeStyle,
};

const PLAYING_FPS: f64 = 10.0;
const PAUSED_FPS: f64 = 0.1;

#[derive(Debug)]
struct MouseCoords {
    x: f64,
    y: f64,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct ImData2 {
    pub found_points: Vec<Point>,
    pub draw_shapes: Vec<CanvasDrawableShape>,
    pub fno: u64,
    pub ts_rfc3339: String, // timestamp in RFC3339 format
    pub ck: bui_backend_types::ConnectionKey,
    pub name: Option<String>,
}

#[derive(Debug, PartialEq, Clone)]
struct LoadedFrame {
    handle: JsValue,
    in_msg: ImData2,
}

pub struct VideoField {
    image: web_sys::HtmlImageElement,
    show_div: bool, // synchronized to whether we are visible
    css_id: String,
    last_frame_render: f64,
    mouse_xy: Option<MouseCoords>,
    green_stroke: StrokeStyle,
    green: JsValue,
    rendered_frame_number: Option<u64>,
    timeout: Option<Timeout>,
}

pub enum Msg {
    FrameLoaded(ImData2),
    NotifySender(FirehoseCallbackInner),
    MouseMove(MouseEvent),
    ToggleCollapsed(bool),
}

#[derive(PartialEq, Properties)]
pub struct Props {
    pub title: String,
    pub video_data: Rc<RefCell<VideoData>>,
    pub width: u32,
    pub height: u32,
    pub measured_fps: f32,
    pub onrendered: Option<Callback<FirehoseCallbackInner>>,
}

impl Component for VideoField {
    type Message = Msg;
    type Properties = Props;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            image: web_sys::HtmlImageElement::new().unwrap_throw(),
            css_id: uuid::Uuid::new_v4().to_string(),
            last_frame_render: 0.0,
            mouse_xy: None,
            show_div: true,
            green_stroke: StrokeStyle::from_rgb(0x7F, 0xFF, 0x7F),
            green: JsValue::from("7fff7f"),
            rendered_frame_number: None,
            timeout: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::MouseMove(mminfo) => {
                let client_x = mminfo.client_x() as f64;
                let client_y = mminfo.client_y() as f64;
                let document = gloo_utils::document();
                let canvas = document.get_element_by_id(&self.css_id).unwrap_throw();
                let canvas: web_sys::HtmlCanvasElement = canvas
                    .dyn_into::<web_sys::HtmlCanvasElement>()
                    .map_err(|_| ())
                    .unwrap_throw();
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
            Msg::FrameLoaded(im_data) => {
                self.draw_frame_canvas(&im_data);

                // Wait before returning request for new frame to throttle view.
                let wait_msecs = {
                    let now = js_sys::Date::now(); // in milliseconds
                    let max_framerate = match self.show_div {
                        true => PLAYING_FPS,
                        false => PAUSED_FPS,
                    };
                    let desired_dt = 1.0 / max_framerate * 1000.0; // convert to msec
                    let desired_now = self.last_frame_render + desired_dt;
                    let wait = desired_now - now;
                    self.last_frame_render = now;
                    wait.round() as i64
                };

                let fno = im_data.fno;
                let fci = FirehoseCallbackInner {
                    ck: im_data.ck,
                    fno: im_data.fno as usize,
                    name: im_data.name.clone(),
                    ts_rfc3339: im_data.ts_rfc3339,
                };

                if wait_msecs > 0 {
                    let millis = wait_msecs as u32;
                    let handle = {
                        let link = ctx.link().clone();
                        Timeout::new(millis, move || link.send_message(Msg::NotifySender(fci)))
                    };
                    self.timeout = Some(handle);
                } else {
                    self.timeout = None;
                    ctx.link().send_message(Msg::NotifySender(fci));
                }

                self.rendered_frame_number = Some(fno);
            }
            Msg::NotifySender(fci) => {
                self.timeout = None;
                if let Some(ref callback) = ctx.props().onrendered {
                    callback.emit(fci);
                }
            }
        }
        false
    }

    fn changed(&mut self, ctx: &Context<Self>, props: &Self::Properties) -> bool {
        let mut video_data = props.video_data.borrow_mut();
        if let Some(in_msg) = video_data.take() {
            // Here we copy the image data. Todo: can we avoid this?
            let data_url = in_msg.firehose_frame_data_url.clone();
            let mut draw_shapes = in_msg.annotations.clone();
            if let Some(ref valid_display) = in_msg.valid_display {
                let line_width = 5.0;
                let green_shape = http_video_streaming_types::DrawableShape::from_shape(
                    valid_display,
                    &self.green_stroke,
                    line_width,
                );
                draw_shapes.push(green_shape);
            }
            let in_msg2 = ImData2 {
                ck: in_msg.ck,
                fno: in_msg.fno,
                found_points: in_msg.found_points.clone(),
                name: in_msg.name.clone(),
                ts_rfc3339: in_msg.ts_rfc3339,
                draw_shapes: draw_shapes.into_iter().map(|s| s.into()).collect(),
            };

            let callback = ctx
                .link()
                .callback(move |_| Msg::FrameLoaded(in_msg2.clone()));

            let on_load_closure = Closure::wrap(Box::new(move || {
                callback.emit(0u8); // dummy arg for callback
            }) as Box<dyn FnMut()>);

            self.image.set_src(&data_url);
            self.image
                .set_onload(Some(on_load_closure.as_ref().unchecked_ref()));
            on_load_closure.forget();
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div class="wrap-collapsible">
              <CheckboxLabel
                label={ctx.props().title.clone()}
                initially_checked={self.show_div}
                oncheck={ctx.link().callback(Msg::ToggleCollapsed)}
                />
              <div>
                <canvas
                    width={format!("{}",ctx.props().width)}
                    height={format!("{}",ctx.props().height)}
                    id={self.css_id.clone()}
                    class="video-field-canvas"
                    onmousemove={ctx.link().callback(Msg::MouseMove)}
                    />
                { self.view_text(ctx) }
              </div>
            </div>
        }
    }
}

impl VideoField {
    fn view_text(&self, ctx: &Context<Self>) -> Html {
        let mouse_str = if let Some(ref mouse_pos) = self.mouse_xy {
            format!("{}, {}", mouse_pos.x as i64, mouse_pos.y as i64)
        } else {
            "".to_string()
        };
        let fno_str = format!("{}", self.rendered_frame_number.unwrap_or(0));
        html! {
            <div class="video-field-text">
                <div class="video-field-fno">{"frame: "}{ &fno_str }</div>
                <div class="video-field-mousepos">{"mouse: "}{ &mouse_str }</div>
                <div class="video-field-fps">
                    {"frames per second: "}{ format!("{:.1}", ctx.props().measured_fps) }
                </div>
            </div>
        }
    }

    fn draw_frame_canvas(&self, in_msg: &ImData2) {
        let document = gloo_utils::document();
        let canvas = document.get_element_by_id(&self.css_id).unwrap_throw();
        let canvas: web_sys::HtmlCanvasElement = canvas
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .map_err(|_| ())
            .unwrap_throw();
        let ctx = web_sys::CanvasRenderingContext2d::from(JsValue::from(
            canvas.get_context("2d").unwrap_throw().unwrap_throw(),
        ));

        ctx.draw_image_with_html_image_element(&self.image, 0.0, 0.0)
            .unwrap_throw();

        ctx.set_stroke_style(&self.green);
        ctx.set_line_width(1.0);

        for pt in in_msg.found_points.iter() {
            ctx.begin_path();
            ctx.arc(
                // circle
                pt.x as f64,
                pt.y as f64,
                30.0,
                0.0,
                std::f64::consts::PI * 2.0,
            )
            .unwrap_throw();

            let r: f64 = 30.0;
            if let Some(theta) = pt.theta {
                let theta = theta as f64;
                let dx = r * theta.cos();
                let dy = r * theta.sin();
                ctx.move_to(pt.x as f64 - dx, pt.y as f64 - dy);
                ctx.line_to(pt.x as f64 + dx, pt.y as f64 + dy);
            }

            ctx.close_path();
            ctx.stroke();
        }

        for drawable_shape in in_msg.draw_shapes.iter() {
            ctx.set_stroke_style(&drawable_shape.stroke_style.clone().into());
            ctx.set_line_width(drawable_shape.line_width as f64);
            use http_video_streaming_types::Shape;
            match &drawable_shape.shape {
                Shape::Everything => {}
                Shape::Circle(circle) => {
                    draw_circle(&ctx, circle);
                }
                Shape::MultipleCircles(circles) => {
                    for circle in circles {
                        draw_circle(&ctx, circle);
                    }
                }
                Shape::Polygon(polygon) => {
                    let p = &polygon.points[..];
                    if p.len() > 1 {
                        ctx.begin_path();

                        ctx.move_to(p[0].0, p[0].1);
                        for pp in &p[1..] {
                            ctx.line_to(pp.0, pp.1);
                        }

                        ctx.close_path();
                        ctx.stroke();
                    }
                }
            }
        }
    }
}

fn draw_circle(ctx: &web_sys::CanvasRenderingContext2d, circle: &CircleParams) {
    ctx.begin_path();
    ctx.arc(
        // circle
        circle.center_x as f64,
        circle.center_y as f64,
        circle.radius as f64,
        0.0,
        std::f64::consts::PI * 2.0,
    )
    .unwrap_throw();
    ctx.close_path();
    ctx.stroke();
}
