use yew::{html, Component, Context, Html, Properties};
use yew_tincture::components::Button;

pub struct ReloadButton {}

pub enum Msg {
    Clicked,
}

#[derive(PartialEq, Properties)]
pub struct Props {
    pub label: String,
}

impl Component for ReloadButton {
    type Message = Msg;
    type Properties = Props;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {}
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Clicked => {
                let window = gloo_utils::window();
                window.location().reload().unwrap();
            }
        }
        false
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <Button
                title={ctx.props().label.clone()}
                onsignal={ctx.link().callback(|_| Msg::Clicked)}
            />
        }
    }
}
