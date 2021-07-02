use super::Button;
use std::fmt;
use yew::prelude::*;

pub struct VecToggle<T>
where
    T: 'static + Clone + PartialEq + fmt::Display + Default,
{
    link: ComponentLink<Self>,
    values: Vec<T>,
    selected_idx: usize,
    onsignal: Option<Callback<usize>>,
}

pub enum Msg {
    Clicked(usize),
}

#[derive(PartialEq, Clone, Properties)]
pub struct Props<T: Clone> {
    pub values: Vec<T>,
    pub selected_idx: usize,
    pub onsignal: Option<Callback<usize>>,
}

// impl<T> Default for Props<T>
// where
//     T: Default,
// {
//     fn default() -> Self {
//         Props {
//             values: Vec::new(),
//             selected_idx: 0,
//             onsignal: None,
//         }
//     }
// }

impl<T> Component for VecToggle<T>
where
    T: 'static + Clone + PartialEq + fmt::Display + Default,
{
    type Message = Msg;
    type Properties = Props<T>;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
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

    fn view(&self) -> Html {
        let all_rendered = self.values.iter().enumerate().map(|(idx, item)| {
            let name = format!("{}", item);
            let is_active = idx == self.selected_idx;
            let disabled = is_active; // do not allow clicking currently active state
            html! {
                <Button
                    title=name
                    disabled=disabled
                    is_active=is_active
                    onsignal=self.link.callback(move |_| Msg::Clicked(idx))
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
