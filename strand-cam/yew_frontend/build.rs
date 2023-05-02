use std::process::Command;

fn git_hash() -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git");
    String::from_utf8(output.stdout)
        .expect("from_utf8")
        .trim()
        .to_string()
}

fn main() {
    let git_rev = git_hash();
    println!("cargo:rustc-env=GIT_HASH={}", git_rev);
}
