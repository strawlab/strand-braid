use super::button::Button;
use serde::{Deserialize, Serialize};
use serde_yaml;
use yew::prelude::*;

pub struct ConfigField<Cfg>
where
    Cfg: Clone + PartialEq + Serialize + 'static,
    for<'de> Cfg: Deserialize<'de>,
{
    link: ComponentLink<Self>,
    local_copy: String,
    parsed_local: Result<Option<Cfg>, serde_yaml::Error>,
    server_version: Option<Cfg>,
    rows: u16,
    onsignal: Option<Callback<String>>,
    local_changes_pending: bool,
}

pub enum Msg {
    OnTextareaInput(String),
    ToBrowser,
    ToServer,
}

#[derive(PartialEq, Clone, Properties)]
pub struct Props<Cfg: Clone> {
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

fn to_string<Cfg>(server_version: &Option<Cfg>) -> String
where
    Cfg: Serialize,
{
    if let Some(ref sv) = server_version {
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

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        let local_copy = to_string(&props.server_version);

        let mut result = Self {
            link,
            local_copy,
            parsed_local: serde_yaml::from_str(""), // result.parsed_local replaced below
            server_version: props.server_version.clone(),
            rows: props.rows,
            onsignal: props.onsignal,
            local_changes_pending: false,
        };
        result.parse_local(); // result.parsed_local replaced here
        result
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::OnTextareaInput(new_local) => {
                self.local_changes_pending = true;
                self.local_copy = new_local;
                self.parse_local();
            }
            Msg::ToBrowser => {
                self.copy_server_to_browser();
            }
            Msg::ToServer => {
                if let Some(ref mut callback) = self.onsignal {
                    callback.emit(self.local_copy.clone());
                }
                self.local_changes_pending = false;
            }
        }
        true
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        self.server_version = props.server_version.clone();
        self.rows = props.rows;
        self.onsignal = props.onsignal;
        if !self.local_changes_pending {
            self.copy_server_to_browser();
        }
        true
    }

    fn view(&self) -> Html {
        let maybe_error_div = match self.parsed_local {
            Ok(ref _v) => {
                html! {
                    <div></div>
                }
            }
            Err(ref e) => {
                let err_str = format!("❌ Error: {:?}", e);
                html! {
                    <div class="config-field-error",>
                        {err_str}
                    </div>
                }
            }
        };
        let server_version_str = to_string(&self.server_version);
        html! {
            <div class="config-field-editor",>
                <div class=("config-field-left-col","config-field-col"),>
                    <div class="config-field-label",>
                        <label>{"Edit configuration"}</label>
                    </div>
                    <div class="config-field-textarea-div",>
                        <textarea rows=self.rows, value=&self.local_copy,
                            class="config-field-textarea",
                            oninput=self.link.callback(|e: InputData| Msg::OnTextareaInput(e.value)),
                            />
                    </div>
                    { maybe_error_div }
                </div>
                <div class=("config-field-middle-col","config-field-col"),>
                    <div class="config-field-btns",>
                        <div class="config-field-btn-to-browser",>
                            <Button: title="<-", onsignal=self.link.callback(|_| Msg::ToBrowser),/>
                        </div>
                        <div class="config-field-btn-to-server",>
                            <Button: title="->", onsignal=self.link.callback(|_| Msg::ToServer),/>
                        </div>
                    </div>
                </div>
                <div class=("config-field-right-col","config-field-col"),>
                    <div class="config-field-label",>
                        <label>{"Current configuration"}</label>
                    </div>
                    <div class="config-field-on-server",>
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

    fn copy_server_to_browser(&mut self) {
        // When the server version changes, update our local copy to it.
        self.parsed_local = Ok(self.server_version.clone());
        self.local_copy = to_string(&self.server_version);
    }
}
