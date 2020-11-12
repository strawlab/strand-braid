extern crate cbindgen;
extern crate plugin_defs; // ensure it is built and up to date

fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

    // Seems to be a bug in cbindgen that this isn't automatic. E.g.
    // https://github.com/eqrion/cbindgen/issues/292 and
    // https://github.com/eqrion/cbindgen/issues/253 and
    // https://github.com/eqrion/cbindgen/issues/127 .

    // Therefore, we do this trick to keep up to date with the real definitions.
    let contents = plugin_defs::PLUGIN_DEFS;

    let mut config: cbindgen::Config = Default::default();
    config.language = cbindgen::Language::C;

    config.header = Some(contents.to_string());
    config.no_includes = true;

    cbindgen::generate_with_config(&crate_dir, config)
        .expect("cbindgen generate_with_config() failed")
        .write_to_file("target/strandcam.h");
}
