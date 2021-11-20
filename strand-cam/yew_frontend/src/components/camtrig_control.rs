use camtrig_comms::{ChannelState, DeviceState, ToDevice};
use yew::prelude::*;
use yew_tincture::components::CheckboxLabel;

use super::led_control::{ChangeLedState, ChangeLedStateValue, LedControl};

pub struct CamtrigControl {}

pub enum Msg {
    LedStateChange(ChangeLedState),
}

#[derive(PartialEq, Properties)]
pub struct Props {
    pub device_state: DeviceState,
    pub onsignal: Option<Callback<ToDevice>>,
}

impl Component for CamtrigControl {
    type Message = Msg;
    type Properties = Props;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {}
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::LedStateChange(command) => {
                if let Some(ref callback) = ctx.props().onsignal {
                    let mut next_state = ctx.props().device_state.clone();
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

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div class="wrap-collapsible">
                <CheckboxLabel label="LED control" initially_checked=true />
                <div>
                    <div class="leds-controllers">
                        <LedControl
                            channel={ctx.props().device_state.ch1.clone()}
                            onsignal={ctx.link().callback(|x| Msg::LedStateChange(x))}
                        />
                        <LedControl
                            channel={ctx.props().device_state.ch2.clone()}
                            onsignal={ctx.link().callback(|x| Msg::LedStateChange(x))}
                        />
                        <LedControl
                            channel={ctx.props().device_state.ch3.clone()}
                            onsignal={ctx.link().callback(|x| Msg::LedStateChange(x))}
                        />
                    </div>
                </div>
            </div>
        }
    }
}
