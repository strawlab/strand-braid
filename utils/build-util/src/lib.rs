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
///   dedicated `trunk-target` subdirectory of `OUT_DIR` and forcing the nested
///   cargo offline (`CARGO_NET_OFFLINE=true`) to avoid deadlocking the outer
///   workspace cargo build on the target-dir and package-cache locks. This
///   requires the wasm32 dependencies to already be in the cargo cache; on a
///   cold cache, pre-fetch them once with
///   `cargo fetch --target wasm32-unknown-unknown`.
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

    // Serialize trunk invocations across the whole machine. Multiple frontend
    // crates (e.g. braid-run and strand-cam) each run `trunk build` from their
    // own build script, and cargo runs build scripts in parallel. On a cold
    // cache (e.g. a fresh CI checkout) every trunk process downloads and
    // extracts the shared wasm-bindgen / wasm-opt tools into the same cache
    // directory (`~/.cache/trunk`) at the same time. Trunk does not lock that
    // step, so one process reads a half-written archive and the build fails
    // with "running wasm-opt -> Could not extract files -> unexpected end of
    // file". Holding this lock for the duration of the build guarantees the
    // tools are fully installed before any other trunk process touches them.
    let _trunk_lock = TrunkBuildLock::acquire()?;

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
        // Force trunk's nested wasm32 cargo invocation to run offline. The
        // outer workspace cargo holds a shared lock on the global package
        // cache (`~/.cargo/.package-cache-mutate`) for the whole build; if the
        // nested cargo tried to *download* a crate it would need an exclusive
        // lock on that same file and block forever, because the outer build is
        // itself blocked waiting for this build script to finish — a deadlock.
        // Running offline means the nested cargo never takes the exclusive
        // lock: with a warm cache (CI with a restored cache, repeat local
        // builds, air-gapped machines) it resolves everything locally and
        // succeeds; on a cold cache it fails fast with a clear error instead
        // of hanging (see the failure message below).
        .env("CARGO_NET_OFFLINE", "true")
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
        return Err(format!(
            "trunk build failed in {frontend_dir} (exit status: {status}).\n\
             The frontend is built by a nested cargo that runs offline (to avoid \
             deadlocking the outer build on the cargo package-cache lock). If the \
             failure above is about missing crates / being unable to download, your \
             cargo cache does not yet contain the wasm32 dependencies. Pre-fetch them \
             once with network access, then rebuild:\n    \
             cargo fetch --target wasm32-unknown-unknown"
        )
        .into());
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

/// A machine-wide, cross-process advisory lock that serializes `trunk build`
/// invocations.
///
/// It is implemented with a lock file created atomically via
/// `create_new`. The lock is released when the guard is dropped (which removes
/// the file). To recover from a build script that crashed while holding the
/// lock, a lock file older than [`Self::STALE_AFTER`] is considered abandoned
/// and is stolen.
struct TrunkBuildLock {
    path: std::path::PathBuf,
}

impl TrunkBuildLock {
    /// A lock file older than this is treated as abandoned by a crashed
    /// process. It is generous: it only needs to exceed the longest plausible
    /// trunk build, never a normal wait.
    const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(30 * 60);
    /// Give up rather than block a build forever if something is wrong.
    const ACQUIRE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

    fn acquire() -> Result<Self, Box<dyn std::error::Error>> {
        use std::io::ErrorKind;

        // A fixed, well-known path so every trunk build script on this machine
        // contends on the same lock.
        let path = std::env::temp_dir().join("strand-braid-trunk-build.lock");
        let start = std::time::Instant::now();

        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_file) => return Ok(Self { path }),
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    // Someone else holds the lock. Steal it if it is stale,
                    // otherwise wait and retry.
                    if Self::is_stale(&path) {
                        // Best effort: if the steal races with the holder
                        // releasing it, we simply retry on the next iteration.
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    if start.elapsed() > Self::ACQUIRE_TIMEOUT {
                        return Err(format!(
                            "timed out after {:?} waiting for the trunk build lock at {}",
                            Self::ACQUIRE_TIMEOUT,
                            path.display()
                        )
                        .into());
                    }
                    std::thread::sleep(Self::POLL_INTERVAL);
                }
                Err(err) => {
                    return Err(format!(
                        "failed to create trunk build lock at {}: {err}",
                        path.display()
                    )
                    .into());
                }
            }
        }
    }

    fn is_stale(path: &std::path::Path) -> bool {
        match std::fs::metadata(path).and_then(|meta| meta.modified()) {
            Ok(modified) => modified.elapsed().unwrap_or_default() > Self::STALE_AFTER,
            // If the file vanished between our open attempt and this check, it is
            // no longer held; treat it as stealable so we retry immediately.
            Err(_) => true,
        }
    }
}

impl Drop for TrunkBuildLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
