use super::Button;
use std::fmt;
use yew::prelude::*;

pub struct VecToggle<T> {
    values: Vec<T>,
    selected_idx: usize,
    onsignal: Option<Callback<usize>>,
}

pub enum Msg {
    Clicked(usize),
}

#[derive(PartialEq, Clone)]
pub struct Props<T> {
    pub values: Vec<T>,
    pub selected_idx: usize,
    pub onsignal: Option<Callback<usize>>,
}

impl<T> Default for Props<T>
where
    T: Default,
{
    fn default() -> Self {
        Props {
            values: Vec::new(),
            selected_idx: 0,
            onsignal: None,
        }
    }
}

impl<T> Component for VecToggle<T>
where
    T: 'static + Clone + PartialEq + Default,
{
    type Message = Msg;
    type Properties = Props<T>;

    fn create(props: Self::Properties, _link: ComponentLink<Self>) -> Self {
        Self {
            values: props.values,
            selected_idx: props.selected_idx,
            onsignal: props.onsignal,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Clicked(selected_idx) => {
                self.selected_idx = selected_idx;
                if let Some(ref callback) = self.onsignal {
                    callback.emit(selected_idx)
                }
            }
        }
        false
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        self.values = props.values;
        self.selected_idx = props.selected_idx;
        self.onsignal = props.onsignal;
        true
    }
}

impl<T> Renderable<VecToggle<T>> for VecToggle<T>
where
    T: 'static + Clone + PartialEq + fmt::Display + Default,
{
    fn view(&self) -> Html<Self> {
        let mut all_rendered = self.values.iter().enumerate().map(|(idx, item)| {
            let name = format!("{}", item);
            let is_active = idx == self.selected_idx;
            let disabled = is_active; // do not allow clicking currently active state
            html! {
                <Button:
                    title=name,
                    disabled=disabled,
                    is_active=is_active,
                    onsignal=move |_| Msg::Clicked(idx),
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
