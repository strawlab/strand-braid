use yew::prelude::*;

pub struct Toggle {
    label: String,
    ontoggle: Option<Callback<bool>>,
    value: bool,
}

pub enum Msg {
    Toggled(bool),
}

#[derive(PartialEq, Clone)]
pub struct Props {
    pub label: String,
    pub ontoggle: Option<Callback<bool>>,
    pub value: bool,
}

impl Default for Props {
    fn default() -> Self {
        Props {
            label: "Send Signal".into(),
            ontoggle: None,
            value: false,
        }
    }
}

impl Component for Toggle {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, _link: ComponentLink<Self>) -> Self {
        Self {
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
}

impl Renderable<Toggle> for Toggle {
    fn view(&self) -> Html<Self> {
        let new_value: bool = !self.value;
        let class = if self.value {
            "toggle-on"
        } else {
            "toggle-off"
        };
        html! {
            <label class=class,>{ &self.label }<input type="checkbox", checked=self.value, onclick=|_| Msg::Toggled(new_value),/></label>
        }
    }
}
