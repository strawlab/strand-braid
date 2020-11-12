use yew::prelude::*;

pub struct Toggle {
    link: ComponentLink<Self>,
    label: String,
    ontoggle: Option<Callback<bool>>,
    value: bool,
}

pub enum Msg {
    Toggled(bool),
}

#[derive(PartialEq, Clone, Properties)]
pub struct Props {
    pub label: String,
    pub ontoggle: Option<Callback<bool>>,
    pub value: bool,
}

impl Component for Toggle {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
            label: props.label,
            ontoggle: props.ontoggle,
            value: props.value,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Toggled(checked) => {
                if let Some(ref mut callback) = self.ontoggle {
                    callback.emit(checked);
                }
            }
        }
        false
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        self.label = props.label;
        self.ontoggle = props.ontoggle;
        self.value = props.value;
        true
    }

    fn view(&self) -> Html {
        let new_value: bool = !self.value;
        let class = if self.value {
            "toggle-on"
        } else {
            "toggle-off"
        };
        html! {
            <label class=class,>{ &self.label }<input type="checkbox", checked=self.value, onclick=self.link.callback(move |_| Msg::Toggled(new_value)),/></label>
        }
    }
}
