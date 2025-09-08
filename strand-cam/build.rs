#[cfg(not(any(feature = "bundle_files", feature = "serve_files")))]
compile_error!("Need cargo feature \"bundle_files\" or \"serve_files\"");

#[cfg(all(feature = "bundle_files", feature = "serve_files"))]
compile_error!(
    "Need exactly one of cargo features \"bundle_files\" or \"serve_files\", but both given."
);

fn main() -> Result<(), Box<(dyn std::error::Error)>> {
    build_util::git_hash(env!("CARGO_PKG_VERSION"))?;

    #[cfg(feature = "bundle_files")]
    {
        let frontend_dir = std::path::PathBuf::from("yew_frontend");
        let frontend_dist_dir = frontend_dir.join("dist");

        if !frontend_dist_dir.join("index.html").exists() {
            return Err(format!(
                "The frontend is required but not present. Hint: go to {} and \
                run `trunk build`.",
                frontend_dir.display()
            )
            .into());
        }
    }

    Ok(())
}
