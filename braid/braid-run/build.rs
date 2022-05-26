fn main() -> Result<(), Box<dyn std::error::Error>> {
    build_util::git_hash(env!("CARGO_PKG_VERSION"))?;

    let frontend_dir = std::path::PathBuf::from("braid_frontend");
    let frontend_pkg_dir = frontend_dir.join("pkg");

    #[cfg(feature = "bundle_files")]
    if !frontend_pkg_dir.join("braid_frontend.js").exists() {
        return Err(format!(
            "The frontend is required but not built. Hint: go to {} and \
            run `wasm-pack build --target web`.",
            frontend_dir.display()
        )
        .into());
    }

    build_util::bui_backend_generate_code(&frontend_pkg_dir, "mainbrain_frontend.rs")?;

    Ok(())
}
