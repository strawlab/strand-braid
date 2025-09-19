fn git_hash(orig_version: &str) -> Result<(), Box<(dyn std::error::Error)>> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()?;
    let git_hash = String::from_utf8(output.stdout)?;
    println!("cargo:rustc-env=GIT_HASH={git_hash}");
    let version = format!("{orig_version}+{git_hash}");
    println!("cargo:rustc-env=CARGO_PKG_VERSION={version}"); // override default
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    git_hash(env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
