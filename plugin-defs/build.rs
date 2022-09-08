use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let header_path = std::path::Path::new(&out_dir).join("plugin-defs.h");

    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut config: cbindgen::Config = Default::default();
    config.language = cbindgen::Language::C;
    config.export.include = vec!["ProcessFrameFunc".to_string()];

    // save header file
    cbindgen::generate_with_config(&crate_dir, config)
        .expect("cbindgen generate_with_config() failed")
        .write_to_file(&header_path);

    // read header file to string
    let mut fd = std::fs::File::open(&header_path).unwrap();
    let mut contents = String::new();
    fd.read_to_string(&mut contents).unwrap();

    // write header as rust file
    let dest_path = Path::new(&out_dir).join("codegen.rs");
    let mut f = File::create(&dest_path).unwrap();
    let line = format!("pub const PLUGIN_DEFS: &'static str = r#\"{}\"#;", contents);
    f.write_all(line.as_bytes()).unwrap();
}
