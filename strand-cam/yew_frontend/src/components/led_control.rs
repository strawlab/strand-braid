use ads_webasm::components::{EnumToggle, RangedValue};
use camtrig_comms::{ChannelState, OnState};
use yew::prelude::*;

const LAST_DETECTED_VALUE_LABEL: &'static str = "Last detected value: ";

pub struct ChangeLedState {
    pub channel_num: u8,
    pub what: ChangeLedStateValue,
}

pub enum ChangeLedStateValue {
    NewOnState(OnState),
    NewIntensity(u16),
}

pub struct LedControl {
    link: ComponentLink<Self>,
    channel: ChannelState,
    onsignal: Option<Callback<ChangeLedState>>,
    pulse_duration_ticks: u16,
}

pub enum Msg {
    Clicked(OnState),
    SetIntensityPercent(f32),
    SetPulseDurationTicks(f32),
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

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
            channel: props.channel,
            onsignal: props.onsignal,
            pulse_duration_ticks: 500,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Clicked(mut on_state) => {
                if let OnState::PulseTrain(ref mut pulse_train_params) = on_state {
                    pulse_train_params.pulse_dur_ticks = self.pulse_duration_ticks.into();
                };
                if let Some(ref mut callback) = self.onsignal {
                    let state = ChangeLedState {
                        channel_num: self.channel.num,
                        what: ChangeLedStateValue::NewOnState(on_state),
                    };
                    callback.emit(state);
                }
            }
            Msg::SetIntensityPercent(percent_value) => {
                if let Some(ref mut callback) = self.onsignal {
                    let state = ChangeLedState {
                        channel_num: self.channel.num,
                        what: ChangeLedStateValue::NewIntensity(
                            (MAX_INTENSITY * percent_value / 100.0) as u16,
                        ),
                    };
                    callback.emit(state);
                }
            }
            Msg::SetPulseDurationTicks(ticks) => {
                self.pulse_duration_ticks = ticks.round() as u16;
            }
        }
        false
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        self.channel = props.channel;
        self.onsignal = props.onsignal;
        true
    }

    fn view(&self) -> Html {
        html! {
            <div class="led-control">
                <h3>{"LED "}{format!("{}", self.channel.num)}</h3>
                <EnumToggle<OnState>
                    value=self.channel.on_state
                    onsignal=self.link.callback(|variant| Msg::Clicked(variant))
                />
                <h3>{"Intensity"}</h3>
                <RangedValue
                    unit="percent"
                    min=0.0
                    max=100.0
                    current=(self.channel.intensity as f32)/MAX_INTENSITY*100.0
                    current_value_label=LAST_DETECTED_VALUE_LABEL
                    placeholder="intensity"
                    onsignal=self.link.callback(|v| {Msg::SetIntensityPercent(v)})
                    />
                <h3>{"Pulse duration"}</h3>
                <RangedValue
                    unit="clock ticks"
                    min=1.0
                    max=std::u16::MAX as f32
                    current=self.pulse_duration_ticks as f32
                    current_value_label=LAST_DETECTED_VALUE_LABEL
                    placeholder="clock ticks"
                    onsignal=self.link.callback(|v| {Msg::SetPulseDurationTicks(v)})
                    />
            </div>
        }
    }
}
