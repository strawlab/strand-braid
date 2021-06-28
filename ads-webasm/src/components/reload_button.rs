use super::button::Button;
use yew::prelude::*;

pub struct ReloadButton {
    link: ComponentLink<Self>,
    label: String,
}

pub enum Msg {
    Clicked,
}

#[derive(PartialEq, Clone, Properties)]
pub struct Props {
    pub label: String,
}

impl Component for ReloadButton {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
            label: props.label,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Clicked => {
                let window = web_sys::window().unwrap();
                window.location().reload().unwrap();
            }
        }
        false
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        self.label = props.label;
        true
    }

    fn view(&self) -> Html {
        html! {
            <Button title=self.label.clone() onsignal=self.link.callback(|_| Msg::Clicked)/>
        }
    }
}
