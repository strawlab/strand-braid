extern crate bui_backend_codegen;

use std::io::Write;
use std::process::Command;

fn git_hash() -> String {
    let output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
        .expect("git");
    String::from_utf8(output.stdout)
        .expect("from_utf8")
        .trim()
        .to_string()
}

fn get_files_dir() -> std::path::PathBuf {
    ["yew_frontend", "pkg"].iter().collect()
}

const BASE: &str = "strand-cam";

#[cfg(feature = "backend_pyloncxx")]
const BACKEND: &str = "pyloncxx";

#[cfg(feature = "backend_flycap2")]
const BACKEND: &str = "flycap2";

#[cfg(feature = "backend_aravis")]
const BACKEND: &str = "aravis";

fn main() {
    #[cfg(not(any(
        feature = "backend_flycap2",
        feature = "backend_pyloncxx",
        feature = "backend_aravis"
    )))]
    compile_error!("no backend selected.");

    let git_rev = git_hash();
    println!("cargo:rustc-env=GIT_HASH={}", git_rev);
    let files_dir = get_files_dir();
    bui_backend_codegen::codegen(&files_dir, "frontend.rs").expect("codegen failed");

    let base_version = env!("CARGO_PKG_VERSION");

    let codegen_fname = "strand-cam-version.json";
    let buf = format!(
        "{{\"version\": \"{}\", \"rev\": \"{}\"}}",
        base_version, git_rev
    );
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest_path = std::path::Path::new(&out_dir) // ./target/debug/build/strand-cam6eb4a8957b2eab09/out/
        .parent()
        .unwrap() // ./target/debug/build/strand-cam6eb4a8957b2eab09
        .parent()
        .unwrap() // ./target/debug/build
        .parent()
        .unwrap() // ./target/debug
        .join(codegen_fname);
    std::fs::File::create(dest_path)
        .unwrap()
        .write_all(buf.as_bytes())
        .unwrap();

    let app_name = format!("{}-{}", BASE, BACKEND);
    println!("cargo:rustc-env=APP_NAME={}", app_name);
}
