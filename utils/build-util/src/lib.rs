// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

/// Set the environment variables `GIT_HASH` AND `CARGO_PKG_VERSION` to include
/// the current git revision.
pub fn git_hash(orig_version: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()?;
    let git_hash = String::from_utf8(output.stdout)?;
    println!("cargo:rustc-env=GIT_HASH={git_hash}");
    let version = format!("{orig_version}+{git_hash}");
    println!("cargo:rustc-env=CARGO_PKG_VERSION={version}"); // override default
    Ok(())
}

/// Build a Trunk-based Yew/WASM frontend crate and verify that the expected
/// output assets are present in `<frontend_dir>/dist`.
///
/// Call this from a `build.rs` behind a `bundle_files` feature gate.
/// `frontend_dir` is the path to the frontend crate relative to the caller's
/// `Cargo.toml` (e.g. `"yew_frontend"` or `"braid_frontend"`).
/// `required_assets` is a list of file names (not paths) that must exist inside
/// `<frontend_dir>/dist` after the build completes (e.g. `&["index.html"]`).
///
/// The function:
/// - Probes for `trunk` and returns a helpful error if it is missing.
/// - Warns if the installed trunk is not the expected 0.21.x series.
/// - Runs `trunk build --release --dist dist` inside `frontend_dir`, using a
///   dedicated `trunk-target` subdirectory of `OUT_DIR` to avoid deadlocking
///   the outer workspace cargo build.
/// - Verifies each required asset is present in the dist directory.
/// - Emits `cargo:rerun-if-changed` directives for the frontend sources,
///   `index.html`, `Trunk.toml`, `scss/`, and the calling `build.rs`.
pub fn trunk_build(
    frontend_dir: &str,
    required_assets: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::ErrorKind;
    use std::path::PathBuf;
    use std::process::Command;

    let out_dir = std::env::var("OUT_DIR")?;
    // Avoid deadlocking with the outer workspace cargo build by using a separate
    // target directory for trunk's nested cargo invocation.
    let trunk_target_dir: PathBuf = PathBuf::from(&out_dir).join("trunk-target");
    std::fs::create_dir_all(&trunk_target_dir)?;

    // frontend_dist_dir is relative to the caller's working directory (i.e. the
    // crate root).  trunk writes its output into frontend_dir/dist.
    let frontend_path = PathBuf::from(frontend_dir);
    let frontend_dist_dir = frontend_path.join("dist");

    // Probe for trunk before attempting a full build so we can surface a
    // helpful install hint rather than an opaque "command not found" error.
    let version_output = match Command::new("trunk").args(["--version"]).output() {
        Ok(output) => output,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err(trunk_missing_error_message().into());
        }
        Err(err) => {
            return Err(format!("Failed to run `trunk --version`: {err}").into());
        }
    };
    if !version_output.status.success() {
        return Err("trunk version check failed".into());
    }

    let version_stdout = String::from_utf8_lossy(&version_output.stdout);
    if !has_trunk_0_21_x(&version_stdout) {
        println!(
            "cargo:warning=Expected trunk version 0.21.x, but found '{}'",
            version_stdout.trim()
        );
    }

    // Build the frontend. `--dist dist` writes output relative to frontend_dir,
    // matching the path the caller expects when embedding files with include_dir.
    let status = match Command::new("trunk")
        .args(["build", "--release", "--dist", "dist"])
        .current_dir(&frontend_path)
        .env("CARGO_TARGET_DIR", &trunk_target_dir)
        // Prevent host-target rustflags (e.g. -C target-cpu=sandybridge) from
        // leaking into trunk's nested wasm32 cargo invocation, where they are
        // unrecognised and silently reset wasm target-features like
        // reference-types, breaking wasm-bindgen.
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .status()
    {
        Ok(status) => status,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err(trunk_missing_error_message().into());
        }
        Err(err) => {
            return Err(format!("Failed to run `trunk build`: {err}").into());
        }
    };

    if !status.success() {
        return Err(
            format!("trunk build failed in {frontend_dir} (exit status: {status}).").into(),
        );
    }

    // Sanity-check that the assets the runtime code expects are actually present.
    for asset in required_assets {
        let asset_path = frontend_dist_dir.join(asset);
        if !asset_path.exists() {
            return Err(format!(
                "Frontend build completed but required asset is missing: {}",
                asset_path.display()
            )
            .into());
        }
    }

    // Re-run only when frontend sources or build script change.
    println!("cargo:rerun-if-changed={frontend_dir}/src");
    println!("cargo:rerun-if-changed={frontend_dir}/scss");
    println!("cargo:rerun-if-changed={frontend_dir}/index.html");
    println!("cargo:rerun-if-changed={frontend_dir}/Trunk.toml");
    println!("cargo:rerun-if-changed=build.rs");

    Ok(())
}

fn trunk_missing_error_message() -> String {
    "`trunk` was not found in PATH, but this build requires it because the \
    `bundle_files` feature is enabled. Install trunk \
    (e.g. `cargo install trunk`) and ensure it is available on PATH before \
    building."
        .to_string()
}

fn has_trunk_0_21_x(version_output: &str) -> bool {
    version_output.split_whitespace().any(|token| {
        token
            .strip_prefix('v')
            .unwrap_or(token)
            .starts_with("0.21.")
    })
}
