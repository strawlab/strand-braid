use yew::{html, Callback, Component, Context, Html, Properties};

pub struct Toggle {}

pub enum Msg {
    Toggled(bool),
}

#[derive(PartialEq, Properties)]
pub struct Props {
    pub label: String,
    pub ontoggle: Option<Callback<bool>>,
    pub value: bool,
}

impl Component for Toggle {
    type Message = Msg;
    type Properties = Props;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {}
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Toggled(checked) => {
                if let Some(ref callback) = ctx.props().ontoggle {
                    callback.emit(checked);
                }
            }
        }
        false
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let new_value: bool = !ctx.props().value;
        let class = if ctx.props().value {
            "toggle-on"
        } else {
            "toggle-off"
        };
        html! {
            <label class={class}>
                { &ctx.props().label }
                <input
                    type="checkbox"
                    checked={ctx.props().value}
                    onclick={ctx.link().callback(move |_| Msg::Toggled(new_value))}
                />
            </label>
        }
    }
}
