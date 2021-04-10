use std::process::Command;

fn git_hash() {
    let output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
        .expect("git");
    let git_hash = String::from_utf8(output.stdout).expect("from_utf8");
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
}

fn main() {
    git_hash();
}
