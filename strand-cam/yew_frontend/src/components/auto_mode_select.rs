use crate::AutoMode;
use ads_webasm::components::EnumToggle;
use yew::prelude::*;

pub struct AutoModeSelect {}

pub enum Msg {
    Clicked(AutoMode),
}

#[derive(PartialEq, Properties)]
pub struct Props {
    pub mode: AutoMode,
    pub onsignal: Option<Callback<AutoMode>>,
}

impl Component for AutoModeSelect {
    type Message = Msg;
    type Properties = Props;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {}
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Clicked(mode) => {
                if let Some(ref callback) = ctx.props().onsignal {
                    callback.emit(mode);
                }
                false // no need to rerender DOM
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div class="auto-mode-container">
                <div class="auto-mode-label">
                    {"Auto mode: "}
                </div>
                <div class="auto-mode-buttons">
                    <EnumToggle<AutoMode>
                        value={ctx.props().mode}
                        onsignal={ctx.link().callback(Msg::Clicked)}
                    />
                </div>
            </div>
        }
    }
}
