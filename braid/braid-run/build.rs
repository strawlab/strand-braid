fn main() -> Result<(), Box<dyn std::error::Error>> {
    build_util::git_hash(env!("CARGO_PKG_VERSION"))?;

    #[cfg(feature = "bundle_files")]
    let frontend_dir = std::path::PathBuf::from("braid_frontend");
    #[cfg(feature = "bundle_files")]
    let frontend_dist_dir = frontend_dir.join("dist");

    #[cfg(feature = "bundle_files")]
    {
        for path in ["braid-logo-no-text.png", "index.html"] {
            if !frontend_dist_dir.join(path).exists() {
                return Err(format!(
                    "The frontend is required but not built. Hint: go to {} and \
                    run `build.sh` (or on Windows, `build.bat`).",
                    frontend_dir.display()
                )
                .into());
            }
        }
    }

    Ok(())
}
