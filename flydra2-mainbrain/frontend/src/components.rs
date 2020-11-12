use seed::prelude::*;
use seed::Listener;

// ------------------------------------------------------------------

pub fn reload_button<Ms>(msg: Ms) -> Node<Ms>
    where
        Ms: Clone,
{
    button![class!["btn", "btn-inactive"],
        "Reload",
        simple_ev(Ev::Click, msg)
    ]
}

// ------------------------------------------------------------------

pub struct RecordingPath {
    value: Option<rust_cam_bui_types::RecordingPath>,
}

impl RecordingPath {
    pub fn new() -> Self {
        Self {
            value: None,
        }
    }

    pub fn set_value(&mut self, value: Option<rust_cam_bui_types::RecordingPath>) {
        self.value = value;
    }

    pub fn view_recording_path<F,Ms>(&self, label: &str, ontoggle: F) -> Node<Ms>
        where
            F: 'static + Fn(bool) -> Ms,
            Ms: Clone,
    {

        let new_value: bool = self.value.is_none();
        let (blinker_class, label_class, widget_inner_class) = if self.value.is_some() {
            ("recording-path-blinker-on",
            "recording-path-label-on",
            "recording-path-widget-inner-on"
            )
        } else {
            ("recording-path-blinker-off",
            "recording-path-label-off",
            "recording-path-widget-inner-off"
            )
        };

        let handler = Box::new(move |_event: web_sys::Event| {
            ontoggle(new_value)
        });

        let listener = Listener::new(
            &Ev::Click.to_string(),
            Some(handler),
            None,
            None,
        );

        let path_disp: Node<Ms> = match self.value {
            Some(ref rp) => {
                let timeval_utc = rp.start_time();

                let jsdate = js_sys::Date::new_0();
                let mins_from_utc = jsdate.get_timezone_offset() as i32;
                let offset = chrono::offset::FixedOffset::west(mins_from_utc*60);
                let timeval = timeval_utc.with_timezone(&offset);

                let buf = format!("Saving to \"{}\", started recording at {}", rp.path(), timeval);
                span![buf]
            }
            None => {
                seed::empty()
            }
        };

        span![
            label![class![label_class], label,
                input![
                    listener,
                    attrs! {
                        At::Type => "checkbox",
                        At::Checked => self.value.is_some(),
                        At::Class => "recording-path-checkbox",
                    },
                ],
                span![
                    class!["recording-path-widget"],
                    span![class!["recording-path-widget-inner",widget_inner_class]]
                ],
            ],
            span![class![blinker_class], span![]],
            path_disp,
        ]
    }
}
