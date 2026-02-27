use web_sys::HtmlInputElement;
use yew::TargetCast;
use yew::{
    events::KeyboardEvent, html, Callback, Component, Context, Html, InputEvent, Properties,
};

pub struct RangedValue {
    local_value_buf: String,
    local_float: Option<f32>,
}

pub enum Msg {
    NewValue(String),
    SendValue,
    Ignore,
}

#[derive(PartialEq, Properties)]
pub struct Props {
    pub unit: String,
    pub min: f32,
    pub max: f32,
    pub current: f32,
    pub placeholder: String,
    pub onsignal: Option<Callback<f32>>,
    pub current_value_label: &'static str,
}

impl Component for RangedValue {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        Self {
            local_value_buf: format!("{}", ctx.props().current),
            local_float: Some(ctx.props().current),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::NewValue(new_value) => {
                self.local_value_buf = new_value;
                match self.local_value_buf.parse() {
                    Ok(vfloat) => {
                        if (ctx.props().min <= vfloat) && (vfloat <= ctx.props().max) {
                            self.local_float = Some(vfloat);
                        } else {
                            self.local_float = None;
                        }
                    }
                    Err(_e) => {
                        self.local_float = None;
                    }
                }
            }
            Msg::SendValue => {
                if let Some(ref callback) = ctx.props().onsignal
                    && let Some(value) = self.local_float {
                        callback.emit(value);
                    }
                return false; // no need to rerender DOM
            }
            Msg::Ignore => {
                return false; // no need to rerender DOM
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let props = ctx.props();
        let current_str = format!("{} ", props.current);
        let range = format!("Range: {} - {} {}", props.min, props.max, props.unit);
        let error = if self.local_float.is_some() {
            ""
        } else {
            "ERROR"
        };
        let input_class = if self.local_float.is_some() {
            "ranged-value-input"
        } else {
            "ranged-value-input-error"
        };
        html! {
            <div class="ranged-value" >
                <div class="ranged-value-state" >
                   { &props.current_value_label }{ current_str }{ props.unit.clone() }
                </div>
                <div class="ranged-value-range" >
                    { range }
                </div>
                <div>
                    <input type="text"
                        class={input_class}
                        placeholder={props.placeholder.clone()}
                        value={self.local_value_buf.clone()}
                        oninput={ctx.link().callback(|e: InputEvent| {
                            let input: HtmlInputElement = e.target_unchecked_into();
                            Msg::NewValue(input.value())
                        })}
                        onblur={ctx.link().callback(|_| Msg::SendValue)}
                        onkeypress={ctx.link().callback(|e: KeyboardEvent| {
                            if e.key() == "Enter" { Msg::SendValue } else { Msg::Ignore }
                        })}
                    />
                    {error}
                </div>
            </div>
        }
    }
}
