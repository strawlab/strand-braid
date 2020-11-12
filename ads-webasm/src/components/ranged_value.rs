use yew::prelude::*;

pub struct RangedValue {
    link: ComponentLink<Self>,
    local_value_buf: String,
    local_float: Option<f32>,
    unit: String,
    min: f32,
    max: f32,
    current: f32,
    placeholder: String,
    onsignal: Option<Callback<f32>>,
    current_value_label: &'static str,
}

pub enum Msg {
    NewValue(String),
    SendValue,
    Ignore,
}

#[derive(PartialEq, Clone, Properties)]
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

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
            local_value_buf: format!("{}", props.current),
            local_float: Some(props.current),
            unit: props.unit,
            min: props.min,
            max: props.max,
            current: props.current,
            placeholder: props.placeholder,
            onsignal: props.onsignal,
            current_value_label: props.current_value_label,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::NewValue(new_value) => {
                self.local_value_buf = new_value;
                match self.local_value_buf.parse() {
                    Ok(vfloat) => {
                        if (self.min <= vfloat) && (vfloat <= self.max) {
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
                if let Some(ref mut callback) = self.onsignal {
                    if let Some(value) = self.local_float {
                        callback.emit(value);
                    }
                }
                return false; // no need to rerender DOM
            }
            Msg::Ignore => {
                return false; // no need to rerender DOM
            }
        }
        true
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        self.unit = props.unit;
        self.min = props.min;
        self.max = props.max;
        self.current = props.current;
        self.placeholder = props.placeholder;
        self.onsignal = props.onsignal;
        self.current_value_label = props.current_value_label;
        true
    }

    fn view(&self) -> Html {
        let current_str = format!("{} ", self.current);
        let range = format!("Range: {} - {} {}", self.min, self.max, self.unit);
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
            <div class="ranged-value",>
                <div class="ranged-value-state",>
                   { &self.current_value_label }{ current_str }{ self.unit.clone() }
                </div>
                <div class="ranged-value-range",>
                    { range }
                </div>
                <div>
                    <input type="text",
                        class=input_class,
                        placeholder=&self.placeholder,
                        value=&self.local_value_buf,
                        oninput=self.link.callback(|e: InputData| Msg::NewValue(e.value)),
                        onblur=self.link.callback(|_| Msg::SendValue),
                        onkeypress=self.link.callback(|e: KeyboardEvent| {
                            if e.key() == "Enter" { Msg::SendValue } else { Msg::Ignore }
                        }), />
                    {error}
                </div>
            </div>
        }
    }
}
