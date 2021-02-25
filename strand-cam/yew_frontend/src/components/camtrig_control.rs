use camtrig_comms::{ChannelState, DeviceState, ToDevice};
use yew::prelude::*;
use yew_tincture::components::CheckboxLabel;

use super::led_control::{ChangeLedState, ChangeLedStateValue, LedControl};

pub struct CamtrigControl {
    link: ComponentLink<Self>,
    device_state: DeviceState,
    onsignal: Option<Callback<ToDevice>>,
}

pub enum Msg {
    LedStateChange(ChangeLedState),
}

#[derive(PartialEq, Clone, Properties)]
pub struct Props {
    pub device_state: DeviceState,
    pub onsignal: Option<Callback<ToDevice>>,
}

impl Component for CamtrigControl {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
            device_state: props.device_state,
            onsignal: props.onsignal,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::LedStateChange(command) => {
                if let Some(ref mut callback) = self.onsignal {
                    let mut next_state = self.device_state.clone();
                    {
                        let mut chan_ref: &mut ChannelState = match command.channel_num {
                            1 => &mut next_state.ch1,
                            2 => &mut next_state.ch2,
                            3 => &mut next_state.ch3,
                            4 => &mut next_state.ch4,
                            _ => panic!("unknown channel"),
                        };
                        match command.what {
                            ChangeLedStateValue::NewOnState(on_state) => {
                                chan_ref.on_state = on_state
                            }
                            ChangeLedStateValue::NewIntensity(intensity) => {
                                chan_ref.intensity = intensity
                            }
                        };
                    }
                    let to_device = ToDevice::DeviceState(next_state);
                    callback.emit(to_device);
                }
            }
        }
        false
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        self.device_state = props.device_state;
        self.onsignal = props.onsignal;
        true
    }

    fn view(&self) -> Html {
        html! {
            <div class="wrap-collapsible",>
                <CheckboxLabel: label="LED control", initially_checked=true, />
                <div>
                    <div class="leds-controllers",>
                        <LedControl:
                            channel=&self.device_state.ch1,
                            onsignal=self.link.callback(|x| Msg::LedStateChange(x)),
                        />
                        <LedControl:
                            channel=&self.device_state.ch2,
                            onsignal=self.link.callback(|x| Msg::LedStateChange(x)),
                        />
                        <LedControl:
                            channel=&self.device_state.ch3,
                            onsignal=self.link.callback(|x| Msg::LedStateChange(x)),
                        />
                    </div>
                </div>
            </div>
        }
    }
}
