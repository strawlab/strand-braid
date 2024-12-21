use wasm_bindgen::prelude::*;
use web_sys::{DragEvent, Event, HtmlInputElement};
use yew::{html, Callback, Component, Context, Html, Properties, TargetCast};

use gloo_file::File;

pub struct FileInput {}

pub enum Msg {
    Files(Vec<File>),
    FileDropped(DragEvent),
    FileDraggedOver(DragEvent),
}

#[derive(Clone, PartialEq, Properties)]
pub struct Props {
    pub button_text: String,
    pub multiple: bool,
    pub accept: String,
    pub on_changed: Option<Callback<Vec<File>>>,
}

impl Component for FileInput {
    type Message = Msg;
    type Properties = Props;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {}
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Files(files) => {
                if let Some(ref callback) = ctx.props().on_changed {
                    callback.emit(files);
                }
            }
            Msg::FileDropped(evt) => {
                evt.prevent_default();
                let files = evt.data_transfer().unwrap_throw().files();
                // log_1(&format!("files dropped: {:?}", files).into());
                if let Some(files) = files {
                    let mut result = Vec::new();
                    let files = js_sys::try_iter(&files)
                        .unwrap_throw()
                        .unwrap_throw()
                        .map(|v| web_sys::File::from(v.unwrap_throw()))
                        .map(File::from);
                    result.extend(files);

                    ctx.link().send_message(Msg::Files(result));
                }
            }
            Msg::FileDraggedOver(evt) => {
                evt.prevent_default();
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let multiple = ctx.props().multiple;
        let accept = ctx.props().accept.clone();
        let button_text = ctx.props().button_text.clone();
        html! {
            <div
                class="custom-file-upload-div"
                ondrop={ctx.link().callback(Msg::FileDropped)}
                ondragover={ctx.link().callback(Msg::FileDraggedOver)}>
                <label class={["btn","custom-file-upload"]}>
                    {button_text}
                        <input
                        type="file"
                        class="custom-file-upload-input"
                        multiple={multiple}
                        accept={accept}
                        onchange={ctx.link().callback(move |e: Event| {
                            let mut result = Vec::new();
                            let input: HtmlInputElement = e.target_unchecked_into();

                            if let Some(files) = input.files() {
                                let files = js_sys::try_iter(&files)
                                    .unwrap()
                                    .unwrap()
                                    .map(|v| web_sys::File::from(v.unwrap()))
                                    .map(File::from);
                                result.extend(files);
                            }
                            Msg::Files(result)
                        })}
                        />

                </label>
            </div>
        }
    }
}
