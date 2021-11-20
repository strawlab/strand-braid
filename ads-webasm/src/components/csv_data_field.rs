use std::collections::HashMap;
use web_sys::{Event, HtmlInputElement};
use yew::{html, Callback, Component, Context, Html, Properties, TargetCast};

use gloo_file::callbacks::FileReader;
use gloo_file::File;

#[derive(PartialEq, Clone)]
pub struct CsvData<RowType> {
    filename: String,
    rows: Vec<RowType>,
    raw_buf: Vec<u8>,
}

impl<RowType> CsvData<RowType> {
    pub fn filename(&self) -> &str {
        &self.filename
    }
    pub fn rows(&self) -> &[RowType] {
        &self.rows
    }
    pub fn raw_buf(&self) -> &[u8] {
        &self.raw_buf
    }
}

pub enum MaybeCsvData<RowType> {
    Valid(CsvData<RowType>),
    Empty,
    ParseFail(String),
}

impl<RowType> std::fmt::Display for MaybeCsvData<RowType> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use self::MaybeCsvData::*;

        match self {
            Valid(ref fd) => write!(
                f,
                "CSV file \"{}\" with {} rows.",
                fd.filename,
                fd.rows.len()
            ),
            Empty => write!(f, "No CSV file loaded."),
            ParseFail(ref _e) => write!(f, "Failed parsing CSV file: {}", _e),
        }
    }
}

pub fn parse_csv<RowType>(filename: String, buf: &[u8]) -> MaybeCsvData<RowType>
where
    for<'de> RowType: serde::Deserialize<'de>,
{
    let raw_buf = buf.to_vec(); // copy raw data
    let rdr = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .from_reader(buf);
    let mut rows = Vec::new();
    for row in rdr.into_deserialize().into_iter() {
        let row: RowType = match row {
            Ok(r) => r,
            Err(e) => return MaybeCsvData::ParseFail(format!("{}", e)),
        };
        rows.push(row);
    }
    MaybeCsvData::Valid(CsvData {
        filename,
        rows,
        raw_buf,
    })
}

impl<RowType> From<Option<CsvData<RowType>>> for MaybeCsvData<RowType> {
    fn from(orig: Option<CsvData<RowType>>) -> MaybeCsvData<RowType> {
        match orig {
            Some(val) => MaybeCsvData::Valid(val),
            None => MaybeCsvData::Empty,
        }
    }
}

pub struct CsvDataField<RowType>
where
    RowType: 'static + Clone + PartialEq,
    for<'de> RowType: serde::Deserialize<'de>,
{
    readers: HashMap<String, FileReader>,
    _row_type: std::marker::PhantomData<RowType>,
}

pub enum Msg {
    Loaded(String, Vec<u8>),
    Files(Vec<File>),
}

#[derive(PartialEq, Properties)]
pub struct Props<RowType>
where
    RowType: PartialEq,
{
    pub onfile: Option<Callback<MaybeCsvData<RowType>>>,
}

impl<RowType> Component for CsvDataField<RowType>
where
    RowType: 'static + Clone + PartialEq,
    for<'de> RowType: serde::Deserialize<'de>,
{
    type Message = Msg;
    type Properties = Props<RowType>;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            readers: HashMap::default(),
            _row_type: std::marker::PhantomData,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Loaded(file_name, data) => {
                self.readers.remove(&file_name);
                let file = parse_csv(file_name, &data);
                if let Some(ref callback) = ctx.props().onfile {
                    callback.emit(file);
                }
            }
            Msg::Files(files) => {
                for file in files.into_iter() {
                    let file_name = file.name();
                    let task = {
                        let file_name = file_name.clone();
                        let link = ctx.link().clone();
                        gloo_file::callbacks::read_as_bytes(&file, move |res| {
                            link.send_message(Msg::Loaded(
                                file_name,
                                res.expect("failed to read file"),
                            ))
                        })
                    };
                    self.readers.insert(file_name, task);
                }
            }
        }
        true
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
                <input type="file"
                    class="custom-file-upload-input"
                    multiple=false
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
        }
    }
}
