use serde::{Deserialize, Serialize};
use web_sys::HtmlInputElement;
use yew::{classes, html, Callback, Component, Context, Html, InputEvent, Properties, TargetCast};
use yew_tincture::components::Button;

pub struct ConfigField<Cfg>
where
    Cfg: Clone + PartialEq + Serialize + 'static,
    for<'de> Cfg: Deserialize<'de>,
{
    local_copy: String,
    parsed_local: Result<Option<Cfg>, serde_yaml::Error>,
    local_changes_pending: bool,
}

pub enum Msg {
    OnTextareaInput(String),
    ToBrowser,
    ToServer,
}

#[derive(PartialEq, Properties)]
pub struct Props<Cfg>
where
    Cfg: PartialEq,
{
    pub server_version: Option<Cfg>,
    pub rows: u16,
    pub onsignal: Option<Callback<String>>,
}

// impl<Cfg> Default for Props<Cfg> {
//     fn default() -> Self {
//         Props {
//             server_version: None,
//             rows: 20,
//             onsignal: None,
//         }
//     }
// }

fn to_string<Cfg>(server_version: Option<&Cfg>) -> String
where
    Cfg: Serialize,
{
    if let Some(sv) = server_version {
        serde_yaml::to_string(sv).unwrap()
    } else {
        // What to do here? This is the case when self.server_version is
        // None, which should not happen except when waiting for initial data.
        "".to_string()
    }
}

impl<Cfg> Component for ConfigField<Cfg>
where
    Cfg: 'static + Clone + PartialEq + Serialize,
    for<'de> Cfg: Deserialize<'de>,
{
    type Message = Msg;
    type Properties = Props<Cfg>;

    fn create(ctx: &Context<Self>) -> Self {
        let local_copy = to_string(ctx.props().server_version.as_ref());

        let mut result = Self {
            local_copy,
            parsed_local: serde_yaml::from_str(""), // result.parsed_local replaced below
            local_changes_pending: false,
        };
        result.parse_local(); // result.parsed_local replaced here
        result
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::OnTextareaInput(new_local) => {
                self.local_changes_pending = true;
                self.local_copy = new_local;
                self.parse_local();
            }
            Msg::ToBrowser => {
                self.copy_server_to_browser(ctx.props().server_version.clone());
            }
            Msg::ToServer => {
                if let Some(ref callback) = ctx.props().onsignal {
                    callback.emit(self.local_copy.clone());
                }
                self.local_changes_pending = false;
            }
        }
        true
    }

    // fn change(&mut self, props: Self::Properties) -> ShouldRender {
    //     self.server_version = props.server_version.clone();
    //     self.rows = props.rows;
    //     self.onsignal = props.onsignal;
    //     if !self.local_changes_pending {
    //         self.copy_server_to_browser(ctx.props().server_version.as_str());
    //     }
    //     true
    // }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let maybe_error_div = match self.parsed_local {
            Ok(ref _v) => {
                html! {
                    <div></div>
                }
            }
            Err(ref e) => {
                let err_str = format!("❌ Error: {:?}", e);
                html! {
                    <div class="config-field-error" >
                        {err_str}
                    </div>
                }
            }
        };
        let server_version_str = to_string(ctx.props().server_version.as_ref());
        html! {
            <div class="config-field-editor" >
                <div class={classes!("config-field-left-col","config-field-col")} >
                    <div class="config-field-label" >
                        <label>{"Edit configuration"}</label>
                    </div>
                    <div class="config-field-textarea-div" >
                        <textarea
                            rows={format!("{}",ctx.props().rows)}
                            value={self.local_copy.clone()}
                            class="config-field-textarea"
                            oninput={ctx.link().callback(|e: InputEvent| {
                                let input: HtmlInputElement = e.target_unchecked_into();
                                Msg::OnTextareaInput(input.value())
                            })}
                            />
                    </div>
                    { maybe_error_div }
                </div>
                <div class={classes!("config-field-middle-col","config-field-col")}>
                    <div class="config-field-btns" >
                        <div class="config-field-btn-to-browser" >
                            <Button
                                title="<-"
                                onsignal={ctx.link().callback(|_| Msg::ToBrowser)}
                            />
                        </div>
                        <div class="config-field-btn-to-server">
                            <Button
                                title="->"
                                onsignal={ctx.link().callback(|_| Msg::ToServer)}
                            />
                        </div>
                    </div>
                </div>
                <div class={classes!("config-field-right-col","config-field-col")}>
                    <div class="config-field-label" >
                        <label>{"Current configuration"}</label>
                    </div>
                    <div class="config-field-on-server">
                        {&server_version_str}
                    </div>
                </div>
            </div>
        }
    }
}

impl<Cfg> ConfigField<Cfg>
where
    Cfg: Serialize + Clone + PartialEq,
    for<'de> Cfg: Deserialize<'de>, // + DeserializeOwned,
{
    fn parse_local(&mut self) {
        self.parsed_local = match serde_yaml::from_str(&self.local_copy) {
            Ok(inner) => Ok(Some(inner)),
            Err(e) => Err(e),
        }
    }

    fn copy_server_to_browser(&mut self, server_version: Option<Cfg>) {
        // When the server version changes, update our local copy to it.
        self.local_copy = to_string(server_version.as_ref());
        self.parsed_local = Ok(server_version);
    }
}
