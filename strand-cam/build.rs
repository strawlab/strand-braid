#[cfg(not(any(feature = "bundle_files", feature = "serve_files")))]
compile_error!("Need cargo feature \"bundle_files\" or \"serve_files\"");

fn main() -> Result<(), Box<(dyn std::error::Error)>> {
    build_util::git_hash(env!("CARGO_PKG_VERSION"))?;

    #[cfg(feature = "bundle_files")]
    {
        let frontend_dir = std::path::PathBuf::from("yew_frontend");
        let frontend_pkg_dir = frontend_dir.join("pkg");

        if !frontend_pkg_dir.join("strand_cam_frontend_yew.js").exists() {
            return Err(format!(
                "The frontend is required but not built. Hint: go to {} and \
                run `build.sh` (or on Windows, `build.bat`).",
                frontend_dir.display()
            )
            .into());
        }
    }

    Ok(())
}
