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

fn main() {
    #[cfg(not(any(feature = "bundle_files", feature = "serve_files")))]
    compile_error!("no file source selected.");

    let git_rev = git_hash();
    println!("cargo:rustc-env=GIT_HASH={}", git_rev);

    let codegen_fname = "braid-version.json";
    let buf = format!(
        "{{\"version\": \"{}\", \"rev\": \"{}\"}}",
        env!("CARGO_PKG_VERSION"),
        git_rev
    );
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest_path = std::path::Path::new(&out_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(codegen_fname);
    std::fs::File::create(dest_path)
        .unwrap()
        .write_all(buf.as_bytes())
        .unwrap();
}
