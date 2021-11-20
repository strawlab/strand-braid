use chrono;
use rust_cam_bui_types::RecordingPath;
use yew::{classes, html, Callback, Component, Context, Html, Properties};

pub struct RecordingPathWidget {}

pub enum Msg {
    Toggled(bool),
}

#[derive(PartialEq, Properties)]
pub struct Props {
    pub label: String,
    pub ontoggle: Option<Callback<bool>>,
    pub value: Option<RecordingPath>,
}

impl Component for RecordingPathWidget {
    type Message = Msg;
    type Properties = Props;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {}
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Toggled(checked) => {
                if let Some(ref callback) = ctx.props().ontoggle {
                    callback.emit(checked);
                }
            }
        }
        false
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let new_value: bool = ctx.props().value.is_none();
        let (blinker_class, label_class, widget_inner_class) = if ctx.props().value.is_some() {
            (
                "recording-path-blinker-on",
                "recording-path-label-on",
                "recording-path-widget-inner-on",
            )
        } else {
            (
                "recording-path-blinker-off",
                "recording-path-label-off",
                "recording-path-widget-inner-off",
            )
        };
        let path_disp = match ctx.props().value {
            Some(ref rp) => {
                let timeval_utc = rp.start_time();

                let jsdate = js_sys::Date::new_0();
                let mins_from_utc_f64: f64 = jsdate.get_timezone_offset();
                let mins_from_utc = mins_from_utc_f64 as i32;
                // check that truncation to i32 does not lose precision
                if (mins_from_utc_f64 - mins_from_utc as f64).abs() > 1e-16 {
                    panic!("Assumption about results of js Date is wrong.");
                }
                let offset = chrono::offset::FixedOffset::west(mins_from_utc * 60);
                let timeval = timeval_utc.with_timezone(&offset);
                html! {
                    <span>
                        { format!("Saving to \"{}\", started recording at {}", rp.path(), timeval) }
                    </span>
                }
            }
            None => {
                html! {
                    <span></span>
                }
            }
        };
        html! {
            <span>
                <label class={label_class} >{ &ctx.props().label }
                    <input type="checkbox"
                        checked={ctx.props().value.is_some()}
                        onclick={ctx.link().callback(move |_| Msg::Toggled(new_value))}
                        class="recording-path-checkbox"
                        />
                    <span class="recording-path-widget">
                        <span class={classes!("recording-path-widget-inner", widget_inner_class)}>
                        </span>
                    </span>
                </label>
                <span class={blinker_class}><span></span></span>
                { path_disp }
            </span>
        }
    }
}
