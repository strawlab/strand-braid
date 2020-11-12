use super::button::Button;
use yew::prelude::*;

pub struct ReloadButton {
    label: String,
}

pub enum Msg {
    Clicked,
}

#[derive(PartialEq, Clone)]
pub struct Props {
    pub label: String,
}

impl Default for Props {
    fn default() -> Self {
        Props {
            label: "Reload".into(),
        }
    }
}

impl Component for ReloadButton {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, _link: ComponentLink<Self>) -> Self {
        Self { label: props.label }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Clicked => {
                js! {
                    window.location.reload();
                }
            }
        }
        false
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        self.label = props.label;
        true
    }
}

impl Renderable<ReloadButton> for ReloadButton {
    fn view(&self) -> Html<Self> {
        html! {
            <Button: title=&self.label, onsignal=|_| Msg::Clicked,/>
        }
    }
}
