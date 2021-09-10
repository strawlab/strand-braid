use super::Button;
use enum_iter::EnumIter;
use serde::{Deserialize, Serialize};
use std::fmt;
use yew::prelude::*;

pub struct EnumToggle<T>
where
    T: EnumIter + Default + Clone + PartialEq + Serialize + 'static + fmt::Display,
    for<'de> T: Deserialize<'de>,
{
    link: ComponentLink<Self>,
    value: T,
    onsignal: Option<Callback<T>>,
}

pub enum Msg<T> {
    Clicked(T),
}

#[derive(PartialEq, Clone, Properties)]
pub struct Props<T>
where
    T: Default + Clone,
{
    pub value: T,
    pub onsignal: Option<Callback<T>>,
}

impl<T> Component for EnumToggle<T>
where
    T: EnumIter + Default + Clone + PartialEq + Serialize + 'static + fmt::Display,
    for<'de> T: Deserialize<'de>,
{
    type Message = Msg<T>;
    type Properties = Props<T>;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
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

    fn view(&self) -> Html {
        let all_rendered = T::variants().iter().map(|variant| {
            let name = format!("{}", variant);
            let is_active = &self.value == variant;
            let disabled = is_active; // do not allow clicking currently active state
            html! {
                <Button
                    title=name
                    disabled=disabled
                    is_active=is_active
                    onsignal=self.link.callback(move |_| Msg::Clicked(variant.clone()))
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
