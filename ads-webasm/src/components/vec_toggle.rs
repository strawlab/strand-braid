use std::fmt;

use yew::{html, Callback, Component, Context, Html, Properties};
use yew_tincture::components::Button;

pub struct VecToggle<T>
where
    T: 'static + Clone + PartialEq + fmt::Display + Default,
{
    _type: std::marker::PhantomData<T>,
}

pub enum Msg {
    Clicked(usize),
}

#[derive(PartialEq, Properties)]
pub struct Props<T>
where
    T: PartialEq,
{
    pub values: Vec<T>,
    pub selected_idx: usize,
    pub onsignal: Option<Callback<usize>>,
}

impl<T> Component for VecToggle<T>
where
    T: 'static + Clone + PartialEq + fmt::Display + Default,
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
            Msg::Clicked(selected_idx) => {
                if let Some(ref callback) = ctx.props().onsignal {
                    callback.emit(selected_idx)
                }
            }
        }
        false
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let all_rendered = ctx.props().values.iter().enumerate().map(|(idx, item)| {
            let name = format!("{}", item);
            let is_active = idx == ctx.props().selected_idx;
            let disabled = is_active; // do not allow clicking currently active state
            html! {
                <Button
                    title={name}
                    disabled={disabled}
                    is_active={is_active}
                    onsignal={ctx.link().callback(move |_| Msg::Clicked(idx))}
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
