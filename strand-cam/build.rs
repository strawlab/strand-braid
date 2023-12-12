fn main() -> Result<(), Box<(dyn std::error::Error)>> {
    build_util::git_hash(env!("CARGO_PKG_VERSION"))?;

    let frontend_dir = std::path::PathBuf::from("yew_frontend");
    let frontend_pkg_dir = frontend_dir.join("pkg");

    #[cfg(feature = "bundle_files")]
    if !frontend_pkg_dir.join("strand_cam_frontend_yew.js").exists() {
        return Err(format!(
            "The frontend is required but not built. Hint: go to {} and \
            run `build.sh` (or on Windows, `build.bat`).",
            frontend_dir.display()
        )
        .into());
    }

    build_util::bui_backend_generate_code(&frontend_pkg_dir, "frontend.rs")?;
    Ok(())
}
