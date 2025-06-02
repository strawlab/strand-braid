use serde::{Deserialize, Serialize};
use std::fmt;
use strand_cam_enum_iter::EnumIter;
use yew::{html, Callback, Component, Context, Html, Properties};
use yew_tincture::components::Button;

pub struct EnumToggle<T>
where
    T: EnumIter + Default + Clone + PartialEq + Serialize + 'static + fmt::Display,
    for<'de> T: Deserialize<'de>,
{
    _type: std::marker::PhantomData<T>,
}

pub enum Msg<T> {
    Clicked(T),
}

#[derive(PartialEq, Properties)]
pub struct Props<T>
where
    T: Default + Clone + PartialEq,
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

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            _type: std::marker::PhantomData,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Clicked(variant) => {
                if let Some(ref callback) = ctx.props().onsignal {
                    callback.emit(variant)
                }
            }
        }
        false
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let all_rendered = T::variants().into_iter().map(|variant| {
            let name = format!("{}", variant);
            let is_active = ctx.props().value == variant;
            let disabled = is_active; // do not allow clicking currently active state
            html! {
                <Button
                    title={name}
                    disabled={disabled}
                    is_active={is_active}
                    onsignal={ctx.link().callback(move |_| Msg::Clicked(variant.clone()))}
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
