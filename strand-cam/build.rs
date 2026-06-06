// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

#[cfg(not(any(feature = "bundle_files", feature = "serve_files")))]
compile_error!("Need cargo feature \"bundle_files\" or \"serve_files\"");

#[cfg(all(feature = "bundle_files", feature = "serve_files"))]
compile_error!(
    "Need exactly one of cargo features \"bundle_files\" or \"serve_files\", but both given."
);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Embed the git hash and date so the binary can report its exact revision.
    build_util::git_hash(env!("CARGO_PKG_VERSION"))?;

    // When `bundle_files` is enabled, compile the Yew/WASM frontend with trunk
    // and embed the resulting assets into the binary via `include_dir`.
    #[cfg(feature = "bundle_files")]
    build_util::trunk_build("yew_frontend", &["index.html"])?;

    Ok(())
}
