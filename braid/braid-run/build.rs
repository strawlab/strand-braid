fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Embed the git hash and date so the binary can report its exact revision.
    build_util::git_hash(env!("CARGO_PKG_VERSION"))?;

    // When `bundle_files` is enabled, compile the Yew/WASM frontend with trunk
    // and embed the resulting assets into the binary via `include_dir`.
    // The required assets (braid-logo-no-text.png and index.html) are verified
    // to exist after the build.
    #[cfg(feature = "bundle_files")]
    build_util::trunk_build("braid_frontend", &["braid-logo-no-text.png", "index.html"])?;

    Ok(())
}
