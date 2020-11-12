use bui_backend_types;
use stdweb;
use video_data::VideoData;
use yew::prelude::*;

use yew_tincture::components::CheckboxLabel;

use http_video_streaming_types::{CanvasDrawableShape, DrawableShape, Point, StrokeStyle};

const PLAYING_FPS: f32 = 10.0;
const PAUSED_FPS: f32 = 0.1;

#[derive(Serialize, Deserialize, Debug)]
struct MouseCoords {
    x: f64,
    y: f64,
}

js_serializable!(MouseCoords);
js_deserializable!(MouseCoords);

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct ImData2 {
    pub found_points: Vec<Point>,
    pub draw_shapes: Vec<CanvasDrawableShape>,
    pub fno: u64,
    pub ts_rfc3339: String, // timestamp in RFC3339 format
    pub ck: bui_backend_types::ConnectionKey,
    pub name: Option<String>,
}

js_serializable!(ImData2);
js_deserializable!(ImData2);

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct LoadedFrame {
    handle: stdweb::Value,
    in_msg: ImData2,
}

js_serializable!(LoadedFrame);
js_deserializable!(LoadedFrame);

pub struct VideoField {
    show_div: bool, // synchronized to whether we are visible
    title: String,
    css_id: String,
    video_data: VideoData,
    last_frame_render_msec: stdweb::Number,
    width: u32,
    height: u32,
    mouse_xy: Option<MouseCoords>,
    measured_fps: f32,
    link: ComponentLink<VideoField>,
    green_stroke: StrokeStyle,
}

pub enum Msg {
    FrameLoaded(stdweb::Value),
    MouseMove(MouseMoveEvent),
    ToggleCollapsed(bool),
}

#[derive(PartialEq, Clone)]
pub struct Props {
    pub title: String,
    pub video_data: VideoData,
    pub width: u32,
    pub height: u32,
    pub measured_fps: f32,
}

impl Default for Props {
    fn default() -> Self {
        Props {
            title: "Video Field".to_string(),
            video_data: VideoData::default(),
            width: 640,
            height: 480,
            measured_fps: 0.0,
        }
    }
}

impl Component for VideoField {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            title: props.title,
            css_id: uuid::Uuid::new_v4().to_string(),
            video_data: props.video_data,
            last_frame_render_msec: stdweb::Number::from(0.0),
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
                let client_x = mminfo.client_x();
                let client_y = mminfo.client_y();
                let value = js! {
                    let clientX = @{client_x};
                    let clientY = @{client_y};
                    let canvas = document.getElementById(@{&self.css_id});
                    let rect = canvas.getBoundingClientRect(); // abs. size of element
                    let scaleX = canvas.width / rect.width;    // relationship bitmap vs. element for X
                    let scaleY = canvas.height / rect.height;  // relationship bitmap vs. element for Y
                    let is_rotate_180 = canvas.classList.contains("rotate-180");
                    let result = {
                        x: (clientX - rect.left) * scaleX,   // scale mouse coordinates after they have
                        y: (clientY - rect.top) * scaleY     // been adjusted to be relative to element
                    };
                    if (is_rotate_180) {
                        result.x = canvas.width - result.x;
                        result.y = canvas.height - result.y;
                    }
                    return result;
                };
                use stdweb::unstable::TryInto;
                let coords: MouseCoords = value.try_into().unwrap();
                self.mouse_xy = Some(coords);
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

                let last_frame_render_msec_value = js! {

                    // TODO:
                    // fix DRY with send_message_buf() by using send_message()
                    // in main?
                    function send_message_buf(buf) {
                        var httpRequest = new XMLHttpRequest();
                        httpRequest.open("POST", "callback");
                        httpRequest.setRequestHeader("Content-Type", "application/json;charset=UTF-8");
                        httpRequest.send(buf);
                    }

                    let handle = @{handle};
                    let img = handle.img;

                    let canvas = document.getElementById(@{&self.css_id});
                    let ctx = canvas.getContext("2d");
                    ctx.drawImage(img, 0, 0);

                    let in_msg = handle.in_msg2;

                    ctx.strokeStyle = "#7FFF7f";
                    ctx.lineWidth = 1.0;

                    in_msg.found_points.forEach(function(pt) {
                        ctx.beginPath();
                        ctx.arc(pt.x, pt.y, 30.0, 0, Math.PI * 2, true); // circle
                        var r = 30.0;
                        if (pt.theta) {
                            var dx = r*Math.cos(pt.theta);
                            var dy = r*Math.sin(pt.theta);
                            ctx.moveTo(pt.x-dx, pt.y-dy);
                            ctx.lineTo(pt.x+dx, pt.y+dy);
                        }
                        ctx.closePath();
                        ctx.stroke();
                    });

                    in_msg.draw_shapes.forEach(function(drawable_shape) {
                        ctx.strokeStyle = drawable_shape.stroke_style;
                        ctx.lineWidth = drawable_shape.line_width;
                        // shape will have either "Circle", "Polygon", or "Everything"
                        var circle = drawable_shape.shape["Circle"];
                        if (typeof circle != "undefined") {
                            ctx.beginPath();
                            ctx.arc(circle.center_x, circle.center_y, circle.radius, 0, Math.PI * 2, true); // circle
                            ctx.closePath();
                            ctx.stroke();
                        }

                        var polygon = drawable_shape.shape["Polygon"];
                        if (typeof polygon != "undefined") {
                            var p = polygon.points;
                            ctx.beginPath();
                            ctx.moveTo(p[0].x, p[1].y);
                            for (i=1; i<p.length; i++) {
                                ctx.lineTo(p[i].x, p[i].y);
                            }
                            ctx.closePath();
                            ctx.stroke();
                        }

                    });

                    let now_msec = Date.now();
                    let wait_msec = 0;
                    let max_framerate = @{rs_max_framerate};
                    let last_frame_render_msec = @{self.last_frame_render_msec};
                    let desired_dt_msec = 1.0/max_framerate*1000.0;
                    let desired_now = last_frame_render_msec+desired_dt_msec;
                    wait_msec = desired_now-now_msec;
                    last_frame_render_msec = now_msec;

                    // Create a FirehoseCallbackInner type
                    let echobuf = JSON.stringify({FirehoseNotify: in_msg});

                    if (wait_msec > 0) {
                        // TODO FIXME XXX: eliminate "window." access to global variable
                        if (!window.is_sleeping) {
                            window.is_sleeping = true;
                            setTimeout(function () {
                                window.is_sleeping = false;
                                send_message_buf(echobuf);
                            },wait_msec);
                        }
                    } else {
                        send_message_buf(echobuf);
                    }

                    return last_frame_render_msec;
                };
                match last_frame_render_msec_value {
                    stdweb::Value::Number(num) => {
                        self.last_frame_render_msec = num;
                    }
                    _ => {
                        panic!("expected number");
                    }
                }
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

            let callback = self.link.send_back(move |v| Msg::FrameLoaded(v));

            // This is typically hidden in a task. Can we make this a Task?
            let callback2 = move |v: stdweb::Value| {
                callback.emit(v);
            };

            js! {
                @(no_return)
                let img = new Image();
                let data_url = @{data_url};
                let in_msg2 = @{in_msg2};
                let jscallback = @{callback2};
                img.src = data_url;
                img.onload = function () {
                    let handle = {
                        img,
                        in_msg2,
                    };

                    jscallback(handle);
                    jscallback.drop();
                };
            };
        }
        true
    }
}

impl Renderable<VideoField> for VideoField {
    fn view(&self) -> Html<Self> {
        html! {
            <div class="wrap-collapsible",>
              <CheckboxLabel:
                label=&self.title,
                initially_checked=self.show_div,
                oncheck=|checked| Msg::ToggleCollapsed(checked),
                />
              <div>
                <canvas width=self.width, height=self.height,
                    id=&self.css_id, class="video-field-canvas",
                    onmousemove=|evt| Msg::MouseMove(evt),
                    />
                { self.view_text() }
              </div>
            </div>
        }
    }
}

impl VideoField {
    fn view_text(&self) -> Html<VideoField> {
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
