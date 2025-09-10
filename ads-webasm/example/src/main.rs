use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use strand_cam_bui_types::RecordingPath;
use yew::prelude::*;

use ads_webasm::components::{
    ConfigField, CsvDataField, EnumToggle, MaybeCsvData, RecordingPathWidget,
};
use yew_tincture::components::CheckboxLabel;
use yew_tincture::components::{Button, RawAndParsed, TypedInput, TypedInputStorage};

enum Msg {
    AddOne,
    SetConfigString(String),
    DoRecordFile(bool),
    SetU8(RawAndParsed<u8>),
    SetF32(RawAndParsed<f32>),
    CsvFile(MaybeCsvData<CsvRowType>),
    ToggleMySelection(MySelection),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct CsvRowType {
    column_a: f32,
    column_b: f32,
    column_c: f32,
    column_d: f32,
}

struct Model {
    csv_file: MaybeCsvData<CsvRowType>,
    cfg: MyConfig,
    raw_u8: TypedInputStorage<u8>,
    raw_f32: TypedInputStorage<f32>,
    record_filename: Option<RecordingPath>,
    my_selection: MySelection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MyConfig {
    value: u8,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
enum MySelection {
    SelOne,
    SelTwo,
    SelThree,
}

impl Default for MySelection {
    fn default() -> MySelection {
        MySelection::SelTwo
    }
}

impl std::fmt::Display for MySelection {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let s = match self {
            MySelection::SelOne => "one",
            MySelection::SelTwo => "two",
            MySelection::SelThree => "three",
        };
        write!(f, "{}", s)
    }
}

impl strand_cam_enum_iter::EnumIter for MySelection {
    fn variants() -> Vec<Self> {
        vec![
            MySelection::SelOne,
            MySelection::SelTwo,
            MySelection::SelThree,
        ]
    }
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        let cfg = MyConfig { value: 123 };
        Self {
            csv_file: MaybeCsvData::Empty,
            cfg,
            raw_u8: TypedInputStorage::empty(),
            raw_f32: TypedInputStorage::empty(),
            record_filename: None,
            my_selection: Default::default(),
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::AddOne => {
                if let Ok(prev) = self.raw_f32.parsed() {
                    self.raw_f32.set_if_not_focused(prev + 1.0);
                }
            }
            Msg::ToggleMySelection(val) => {
                self.my_selection = val;
            }
            Msg::CsvFile(csv_file) => {
                self.csv_file = csv_file;
            }
            Msg::DoRecordFile(is_recording) => {
                if is_recording {
                    // chrono::Utc::now() panics in wasm. So we create it this
                    // way.
                    let jsdate = js_sys::Date::new_0();
                    let iso8601_dt_str = jsdate.to_iso_string();
                    let created_at: Option<chrono::DateTime<chrono::Utc>> =
                        chrono::DateTime::parse_from_rfc3339(&iso8601_dt_str.as_string().unwrap())
                            .ok()
                            .map(|dt| dt.with_timezone(&chrono::Utc));
                    let utc = created_at.unwrap();
                    self.record_filename = Some(RecordingPath::from_path_and_time(
                        "filename".to_string(),
                        utc,
                    ));
                } else {
                    self.record_filename = None;
                }
            }
            Msg::SetConfigString(yaml_buf) => match serde_yaml::from_str(&yaml_buf) {
                Ok(cfg) => self.cfg = cfg,
                Err(_) => {}
            },
            Msg::SetU8(_v) => {
                // we could do something with success or failure value.
            }
            Msg::SetF32(_v) => {
                // we could do something with success or failure value.
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let csv_file_state = format!("{}", self.csv_file);

        // It would be nice to make a `Collapsible` class, but this requires a
        // yew component supporting children. Right now, they apparently do
        // not: https://github.com/DenisKolodin/yew/issues/256

        html! {
            <div>
               {"Hello from rust"}



               <div>
                    <EnumToggle<MySelection>
                        value={self.my_selection.clone()}
                        onsignal={ctx.link().callback(|val| {Msg::ToggleMySelection(val)})}
                    />
               </div>


               <div>
                   <RecordingPathWidget
                       label="Record file directory"
                       value={self.record_filename.clone()}
                       ontoggle={ctx.link().callback(|checked| {Msg::DoRecordFile(checked)})}
                       />
               </div>


               <Button title="Add 1.0 to f32 float" onsignal={ctx.link().callback(|_| Msg::AddOne)}/>

               <div>
                   <label>{"u8 int"}
                   <TypedInput<u8>
                       storage={self.raw_u8.clone()}
                       on_input={ctx.link().callback(|v| Msg::SetU8(v))}
                       />
                   </label>
               </div>


               <div>
                   <label>{"f32 float"}
                   <TypedInput<f32>
                       storage={self.raw_f32.clone()}
                       on_input={ctx.link().callback(|v| Msg::SetF32(v))}
                       />
                   </label>
               </div>


               <div>
                   <h2>{"Data Upload"}</h2>
                    <CsvDataField<CsvRowType>
                        button_text="Select a CSV file."
                        onfile={ctx.link().callback(|csv_file| Msg::CsvFile(csv_file))}
                        />
                   <p>
                       { &csv_file_state }
                   </p>

               </div>


               <div>
                   <ConfigField<MyConfig>
                        server_version={Some(self.cfg.clone())}
                        rows=20
                        onsignal={ctx.link().callback(|s| {Msg::SetConfigString(s)})}
                        />
               </div>


               <div class="wrap-collapsible">
                   <CheckboxLabel label="Label 1" />
                   <div>
                      {"Content that should be hidden by default"}
                   </div>

               </div>


               <div class="wrap-collapsible">
                   <CheckboxLabel label="Label 2" initially_checked=true />
                   <div>
                      {"Content that should be shown by default"}
                   </div>

               </div>


               <div>
                   <input id="unique3" type="checkbox" />
                   <label for="unique3">{"Label 3"}</label>
                   <div>
                      {"Content that should always be shown."}
                   </div>
               </div>

            </div>
        }
    }
}

pub fn main() -> Result<(), JsValue> {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<Model>::new().render();
    Ok(())
}
