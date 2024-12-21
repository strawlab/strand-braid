use std::collections::HashMap;
use yew::{html, Callback, Component, Context, Html, Properties};

use gloo_file::callbacks::FileReader;
use gloo_file::File;

use crate::components::file_input::FileInput;

pub struct ObjWidget {
    readers: HashMap<String, FileReader>,
}

pub enum Msg {
    Loaded(String, Vec<u8>),
    Files(Vec<File>),
}

pub enum MaybeValidObjFile {
    NotLoaded,
    ParseFail(simple_obj_parse::Error),
    Valid(Box<ValidObjFile>),
    NotExactlyOneMesh,
}

impl std::fmt::Display for MaybeValidObjFile {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use self::MaybeValidObjFile::*;

        match self {
            Valid(ref fd) => write!(
                f,
                "OBJ file \"{}\" with {} vertices.",
                fd.filename,
                fd.mesh.coords.len()
            ),
            NotLoaded => write!(f, "No OBJ file loaded."),
            ParseFail(ref _e) => write!(f, "Failed parsing OBJ file."),
            NotExactlyOneMesh => write!(f, "OBJ file loaded, but not exactly one mesh present."),
        }
    }
}

pub struct ValidObjFile {
    pub filename: String,
    _filesize: usize,
    _meshname: String,
    mesh: textured_tri_mesh::TriMesh,
}

impl ValidObjFile {
    pub fn mesh(&self) -> &textured_tri_mesh::TriMesh {
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
    pub button_text: String,
    pub onfile: Option<Callback<MaybeValidObjFile>>,
}

impl Component for ObjWidget {
    type Message = Msg;
    type Properties = Props;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            readers: HashMap::default(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Loaded(filename, buf) => {
                self.readers.remove(&filename);
                let file = match simple_obj_parse::obj_parse(&buf) {
                    Ok(obj_list) => {
                        if obj_list.len() == 1 {
                            let obj = obj_list.into_iter().next().unwrap();

                            MaybeValidObjFile::Valid(Box::new(ValidObjFile {
                                filename,
                                _filesize: buf.len(),
                                _meshname: obj.0,
                                mesh: obj.1,
                            }))
                        } else {
                            MaybeValidObjFile::NotExactlyOneMesh
                        }
                    }
                    Err(e) => MaybeValidObjFile::ParseFail(e),
                };

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
        let button_text = ctx.props().button_text.clone();
        html! {
            <FileInput
                button_text={button_text}
                multiple=false
                accept=".obj"
                on_changed={ctx.link().callback(|files| {
                    Msg::Files(files)
                })}
                />
        }
    }
}
