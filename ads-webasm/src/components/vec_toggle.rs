use std::fmt;

use yew::{html, Callback, Component, Context, Html, Properties};
use yew_tincture::components::Button;

pub struct VecToggle<T>
where
    T: 'static + Clone + PartialEq + fmt::Display,
{
    _type: std::marker::PhantomData<T>,
}

pub enum Msg {
    Clicked(String),
}

#[derive(PartialEq, Properties)]
pub struct Props<T>
where
    T: PartialEq,
{
    pub values: Vec<T>,
    pub selected: Option<String>,
    pub onsignal: Option<Callback<String>>,
}

impl<T> Component for VecToggle<T>
where
    T: 'static + Clone + PartialEq + fmt::Display,
{
    type Message = Msg;
    type Properties = Props<T>;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            _type: std::marker::PhantomData,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Clicked(selected) => {
                if let Some(ref callback) = ctx.props().onsignal {
                    callback.emit(selected)
                }
            }
        }
        false
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let all_rendered = ctx.props().values.iter().map(|item| {
            let name = format!("{}", item);
            let is_active = Some(&name) == ctx.props().selected.as_ref();
            let disabled = is_active; // do not allow clicking currently active state
            let name2 = name.clone();
            html! {
                <Button
                    title={name2}
                    disabled={disabled}
                    is_active={is_active}
                    onsignal={ctx.link().callback(move |_| Msg::Clicked(name.clone()))}
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
