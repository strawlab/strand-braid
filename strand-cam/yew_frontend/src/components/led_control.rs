use ads_webasm::components::{EnumToggle, RangedValue};
use led_box_comms::{ChannelState, OnState};
use yew::prelude::*;

const LAST_DETECTED_VALUE_LABEL: &str = "Last detected value: ";

pub struct ChangeLedState {
    pub channel_num: u8,
    pub what: ChangeLedStateValue,
}

pub enum ChangeLedStateValue {
    NewOnState(OnState),
    NewIntensity(u16),
}

pub struct LedControl {}

pub enum Msg {
    Clicked(OnState),
    SetIntensityPercent(f32),
}

#[derive(PartialEq, Clone, Properties)]
pub struct Props {
    pub channel: ChannelState,
    pub onsignal: Option<Callback<ChangeLedState>>,
}

// TODO Hmm not sure the origin of this number.... Max counter value?
const MAX_INTENSITY: f32 = 16032.0;

impl Component for LedControl {
    type Message = Msg;
    type Properties = Props;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {}
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Clicked(on_state) => {
                if let Some(ref callback) = ctx.props().onsignal {
                    let state = ChangeLedState {
                        channel_num: ctx.props().channel.num,
                        what: ChangeLedStateValue::NewOnState(on_state),
                    };
                    callback.emit(state);
                }
            }
            Msg::SetIntensityPercent(percent_value) => {
                if let Some(ref callback) = ctx.props().onsignal {
                    let state = ChangeLedState {
                        channel_num: ctx.props().channel.num,
                        what: ChangeLedStateValue::NewIntensity(
                            (MAX_INTENSITY * percent_value / 100.0) as u16,
                        ),
                    };
                    callback.emit(state);
                }
            }
        }
        false
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div class="led-control">
                <h3>{"LED "}{format!("{}", ctx.props().channel.num)}</h3>
                <EnumToggle<OnState>
                    value={ctx.props().channel.on_state}
                    onsignal={ctx.link().callback(Msg::Clicked)}
                />
                <h3>{"Intensity"}</h3>
                <RangedValue
                    unit="percent"
                    min=0.0
                    max=100.0
                    current={(ctx.props().channel.intensity as f32)/MAX_INTENSITY*100.0}
                    current_value_label={LAST_DETECTED_VALUE_LABEL}
                    placeholder="intensity"
                    onsignal={ctx.link().callback(|v| {Msg::SetIntensityPercent(v)})}
                    />
            </div>
        }
    }
}
