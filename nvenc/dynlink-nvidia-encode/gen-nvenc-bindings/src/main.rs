use bindgen;

use std::io::Write;

#[derive(Debug)]
pub struct Fix753 {}
impl bindgen::callbacks::ParseCallbacks for Fix753 {
    fn item_name(&self, original_item_name: &str) -> Option<String> {
        Some(original_item_name.trim_start_matches("Fix753_").to_owned())
    }
}

fn main() {
    let mut args = std::env::args_os();
    let _me = args.next().unwrap();
    let dest_fname = args.next().expect("must specify exactly one filename");
    assert!(
        args.next().is_none(),
        "must specify exactly one filename, you specified more than one"
    );
    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .parse_callbacks(Box::new(Fix753 {}))
        .whitelist_function("NvEncodeAPI.*")
        .whitelist_type("NV.*")
        .whitelist_type("_NV.*")
        .whitelist_type("PNV.*")
        .whitelist_var("Fix753.*")
        .default_enum_style(bindgen::EnumVariation::ModuleConsts)
        .derive_debug(false)
        .derive_eq(true)
        .raw_line("#![allow(dead_code,non_upper_case_globals,non_camel_case_types,non_snake_case)]")
        .generate()
        .expect("Unable to generate bindings");

    let mut fd = std::fs::File::create(&dest_fname).expect("unable to generate file");
    fd.write(bindings.to_string().as_bytes())
        .expect("cannot write");
}
