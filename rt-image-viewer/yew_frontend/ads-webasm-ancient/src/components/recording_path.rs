use chrono;
use rust_cam_bui_types::RecordingPath;
use stdweb;
use yew::prelude::*;

pub struct RecordingPathWidget {
    label: String,
    ontoggle: Option<Callback<bool>>,
    value: Option<RecordingPath>,
}

pub enum Msg {
    Toggled(bool),
}

#[derive(PartialEq, Clone)]
pub struct Props {
    pub label: String,
    pub ontoggle: Option<Callback<bool>>,
    pub value: Option<RecordingPath>,
}

impl Default for Props {
    fn default() -> Self {
        Props {
            label: "Recording Path".into(),
            ontoggle: None,
            value: None,
        }
    }
}

impl Component for RecordingPathWidget {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, _link: ComponentLink<Self>) -> Self {
        Self {
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
}

impl Renderable<RecordingPathWidget> for RecordingPathWidget {
    fn view(&self) -> Html<Self> {
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

                let jsdate = stdweb::web::Date::new();
                let mins_from_utc: i32 = jsdate.get_timezone_offset();
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
                <label class=label_class,>{ &self.label }
                    <input type="checkbox",
                        checked=self.value.is_some(),
                        onclick=|_| Msg::Toggled(new_value),
                        class="recording-path-checkbox",
                        />
                    <span class="recording-path-widget",>
                        <span class=("recording-path-widget-inner",widget_inner_class),>
                        </span>
                    </span>
                </label>
                <span class=blinker_class,><span></span></span>
                { path_disp }
            </span>
        }
    }
}
