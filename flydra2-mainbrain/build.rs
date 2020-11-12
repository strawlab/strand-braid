use std::process::Command;
extern crate bui_backend_codegen;

fn get_files_dir() -> std::path::PathBuf {
    ["frontend", "pkg"].iter().collect()
}

fn git_hash() {
    let output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
        .expect("git");
    let git_hash = String::from_utf8(output.stdout).expect("from_utf8");
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
    let version = format!("{}+{}", env!("CARGO_PKG_VERSION"), git_hash);
    println!("cargo:rustc-env=CARGO_PKG_VERSION={}", version); // override default
}

fn main() {
    git_hash();

    let files_dir = get_files_dir();
    bui_backend_codegen::codegen(&files_dir, "mainbrain_frontend.rs").expect("codegen failed");
}
