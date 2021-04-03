use crate::AutoMode;
use ads_webasm::components::EnumToggle;
use yew::prelude::*;

pub struct AutoModeSelect {
    link: ComponentLink<Self>,
    mode: AutoMode,
    onsignal: Option<Callback<AutoMode>>,
}

pub enum Msg {
    Clicked(AutoMode),
}

#[derive(PartialEq, Clone, Properties)]
pub struct Props {
    pub mode: AutoMode,
    pub onsignal: Option<Callback<AutoMode>>,
}

impl Component for AutoModeSelect {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
            mode: props.mode,
            onsignal: props.onsignal,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Clicked(mode) => {
                if let Some(ref mut callback) = self.onsignal {
                    callback.emit(mode);
                }
                return false; // no need to rerender DOM
            }
        }
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        self.mode = props.mode;
        self.onsignal = props.onsignal;
        true
    }

    fn view(&self) -> Html {
        html! {
            <div class="auto-mode-container",>
                <div class="auto-mode-label",>
                    {"Auto mode: "}
                </div>
                <div class="auto-mode-buttons",>
                    <EnumToggle<AutoMode>:
                        value=self.mode,
                        onsignal=self.link.callback(|variant| Msg::Clicked(variant)),
                    />
                </div>
            </div>
        }
    }
}
