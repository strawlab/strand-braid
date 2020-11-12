use super::Button;
use enum_iter::EnumIter;
use std::fmt;
use yew::prelude::*;

pub struct EnumToggle<T> {
    value: T,
    onsignal: Option<Callback<T>>,
}

pub enum Msg<T> {
    Clicked(T),
}

#[derive(PartialEq, Clone)]
pub struct Props<T> {
    pub value: T,
    pub onsignal: Option<Callback<T>>,
}

impl<T> Default for Props<T>
where
    T: Default,
{
    fn default() -> Self {
        Props {
            value: T::default(),
            onsignal: None,
        }
    }
}

impl<T> Component for EnumToggle<T>
where
    T: 'static + Clone + PartialEq + Default,
{
    type Message = Msg<T>;
    type Properties = Props<T>;

    fn create(props: Self::Properties, _link: ComponentLink<Self>) -> Self {
        Self {
            value: props.value,
            onsignal: props.onsignal,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Clicked(variant) => {
                if let Some(ref callback) = self.onsignal {
                    callback.emit(variant)
                }
            }
        }
        false
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        self.value = props.value;
        self.onsignal = props.onsignal;
        true
    }
}

impl<T> Renderable<EnumToggle<T>> for EnumToggle<T>
where
    T: 'static + Clone + PartialEq + fmt::Display + Default + EnumIter,
{
    fn view(&self) -> Html<Self> {
        let mut all_rendered = T::variants().into_iter().map(|variant| {
            let name = format!("{}", variant);
            let is_active = &self.value == variant;
            let disabled = is_active; // do not allow clicking currently active state
            html! {
                <Button:
                    title=name,
                    disabled=disabled,
                    is_active=is_active,
                    onsignal=move |_| Msg::Clicked(variant.clone()),
                />
            }
        });
        html! {
            <div>
                { for all_rendered }
            </div>
        }
    }
}
