use std::process::Command;

fn git_hash() -> (String, String) {
    let output = Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
        .expect("git");
    let git_rev = String::from_utf8(output.stdout)
        .expect("from_utf8")
        .trim()
        .to_string();

    let output = Command::new("git")
        .args(&["show", "--no-patch", "--no-notes", "--pretty=%cd"])
        .output()
        .expect("git");
    let git_date = String::from_utf8(output.stdout)
        .expect("from_utf8")
        .trim()
        .to_string();
    (git_rev, git_date)
}

fn main() {
    let (git_rev, git_date) = git_hash();
    println!("cargo:rustc-env=GIT_HASH={}", git_rev);
    println!("cargo:rustc-env=GIT_DATE={}", git_date);
}
