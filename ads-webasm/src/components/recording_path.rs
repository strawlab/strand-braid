use chrono;
use rust_cam_bui_types::RecordingPath;
use yew::prelude::*;

pub struct RecordingPathWidget {
    link: ComponentLink<Self>,
    label: String,
    ontoggle: Option<Callback<bool>>,
    value: Option<RecordingPath>,
}

pub enum Msg {
    Toggled(bool),
}

#[derive(PartialEq, Clone, Properties)]
pub struct Props {
    pub label: String,
    pub ontoggle: Option<Callback<bool>>,
    pub value: Option<RecordingPath>,
}

impl Component for RecordingPathWidget {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
            label: props.label,
            ontoggle: props.ontoggle,
            value: props.value,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Toggled(checked) => {
                if let Some(ref mut callback) = self.ontoggle {
                    callback.emit(checked);
                }
            }
        }
        false
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        self.label = props.label;
        self.ontoggle = props.ontoggle;
        self.value = props.value;
        true
    }

    fn view(&self) -> Html {
        let new_value: bool = self.value.is_none();
        let (blinker_class, label_class, widget_inner_class) = if self.value.is_some() {
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
        let path_disp = match self.value {
            Some(ref rp) => {
                let timeval_utc = rp.start_time();

                let jsdate = js_sys::Date::new_0();
                let mins_from_utc_f64: f64 = jsdate.get_timezone_offset();
                let mins_from_utc = mins_from_utc_f64 as i32;
                assert_eq!(mins_from_utc_f64, mins_from_utc as f64);
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
                <label class=label_class >{ &self.label }
                    <input type="checkbox"
                        checked=self.value.is_some()
                        onclick=self.link.callback(move |_| Msg::Toggled(new_value))
                        class="recording-path-checkbox"
                        />
                    <span class="recording-path-widget" >
                        <span class=classes!("recording-path-widget-inner", widget_inner_class)>
                        </span>
                    </span>
                </label>
                <span class=blinker_class><span></span></span>
                { path_disp }
            </span>
        }
    }
}
