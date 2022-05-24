extern crate bui_backend_codegen;

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

fn main() -> Result<(), Box<(dyn std::error::Error)>> {
    let git_rev = git_hash();
    println!("cargo:rustc-env=GIT_HASH={}", git_rev);
    let files_dir = get_files_dir();
    let generated_path = "frontend.rs";
    match bui_backend_codegen::codegen(&files_dir, &generated_path) {
        Ok(()) => Ok(()),
        Err(e) => Err(format!(
            "Error in the process of generating '{generated_path}' when attempting to read {} : {e}",
            files_dir.display()
        )
        .into()),
    }
}
