use yew::prelude::*;
use yew::services::reader::{File, FileData, ReaderService, ReaderTask};

pub struct ObjWidget {
    link: ComponentLink<Self>,
    reader: ReaderService,
    tasks: Vec<ReaderTask>,
    onfile: Option<Callback<MaybeValidObjFile>>,
}

pub enum Msg {
    Loaded(FileData),
    Files(Vec<File>),
}

pub enum MaybeValidObjFile {
    NotLoaded,
    ParseFail(simple_obj_parse::Error),
    Valid(ValidObjFile),
    NotExactlyOneMesh,
}

impl std::fmt::Display for MaybeValidObjFile {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use self::MaybeValidObjFile::*;

        match &self {
            &Valid(ref fd) => write!(
                f,
                "OBJ file \"{}\" with {} vertices.",
                fd.filename,
                fd.mesh.vertices().len()
            ),
            &NotLoaded => write!(f, "No OBJ file loaded."),
            &ParseFail(ref _e) => write!(f, "Failed parsing OBJ file."),
            &NotExactlyOneMesh => write!(f, "OBJ file loaded, but not exactly one mesh present."),
        }
    }
}

pub struct ValidObjFile {
    pub filename: String,
    _filesize: usize,
    _meshname: String,
    mesh: ncollide3d::shape::TriMesh<f64>,
}

impl ValidObjFile {
    pub fn mesh(&self) -> &ncollide3d::shape::TriMesh<f64> {
        &self.mesh
    }
}

impl Default for MaybeValidObjFile {
    fn default() -> Self {
        MaybeValidObjFile::NotLoaded
    }
}

#[derive(Clone, PartialEq, Properties)]
pub struct Props {
    pub onfile: Option<Callback<MaybeValidObjFile>>,
}

impl Component for ObjWidget {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
            reader: ReaderService::new(),
            tasks: vec![],
            onfile: props.onfile,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Loaded(file) => {
                let filename = file.name;

                let buf = file.content;
                let file = match simple_obj_parse::obj_parse(&buf) {
                    Ok(obj_list) => {
                        if obj_list.len() == 1 {
                            let obj = obj_list.into_iter().next().unwrap();

                            MaybeValidObjFile::Valid(ValidObjFile {
                                filename,
                                _filesize: buf.len(),
                                _meshname: obj.0,
                                mesh: obj.1,
                            })
                        } else {
                            MaybeValidObjFile::NotExactlyOneMesh
                        }
                    }
                    Err(e) => MaybeValidObjFile::ParseFail(e),
                };

                if let Some(ref mut callback) = self.onfile {
                    callback.emit(file);
                }
            }
            Msg::Files(files) => {
                for file in files.into_iter() {
                    let callback = self.link.callback(Msg::Loaded);
                    let task = self.reader.read_file(file, callback).unwrap();
                    self.tasks.push(task);
                }
            }
        }
        true
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        self.onfile = props.onfile;
        true
    }

    fn view(&self) -> Html {
        html! {
            <input type="file",
                class="custom-file-upload-input",
                multiple=false,
                accept=".obj",
                onchange=self.link.callback(|value| {
                    let mut result = Vec::new();
                    if let ChangeData::Files(files) = value {
                        let files = js_sys::try_iter(&files)
                            .unwrap()
                            .unwrap()
                            .into_iter()
                            .map(|v| File::from(v.unwrap()));
                        result.extend(files);
                    }
                    Msg::Files(result)
                }),
                />
        }
    }
}
