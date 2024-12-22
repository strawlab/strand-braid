use wasm_bindgen::prelude::*;
use web_sys::{DragEvent, Event, HtmlInputElement};
use yew::{html, Callback, Component, Context, Html, Properties, TargetCast};

use gloo_file::File;

pub struct FileInput {
    enter_count: u16,
}

pub enum Msg {
    DragEnter(DragEvent),
    DragLeave(DragEvent),
    DragOver(DragEvent),
    Drop(DragEvent),
    Files(Vec<File>),
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
        Self { enter_count: 0 }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::DragEnter(_evt) => self.enter_count += 1,
            Msg::DragLeave(_evt) => self.enter_count -= 1,
            Msg::Files(files) => {
                if let Some(ref callback) = ctx.props().on_changed {
                    callback.emit(files);
                }
            }
            Msg::Drop(evt) => {
                evt.prevent_default();
                self.enter_count = 0;
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
            Msg::DragOver(evt) => {
                evt.prevent_default();
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let multiple = ctx.props().multiple;
        let accept = ctx.props().accept.clone();
        let button_text = ctx.props().button_text.clone();
        let outer_classes: &[&'static str] = if self.enter_count > 0 {
            &[
                "custom-file-upload-div-outer",
                "custom-file-upload-dropzone",
            ]
        } else {
            &["custom-file-upload-div-outer"]
        };
        html! {
            <div
                class={outer_classes}
                ondrop={ctx.link().callback(Msg::Drop)}
                ondragenter={ctx.link().callback(Msg::DragEnter)}
                ondragleave={ctx.link().callback(Msg::DragLeave)}
                ondragover={ctx.link().callback(Msg::DragOver)}
                >
                <div class="custom-file-upload-fs">{"File drop zone"}</div>
                <div class="custom-file-upload-div-inner">
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
            </div>
        }
    }
}
