// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

/// Environment variable used to supply the git revision when building outside a
/// git checkout (e.g. from a source archive), where `git rev-parse HEAD` yields
/// nothing.
const GIT_HASH_OVERRIDE_ENV: &str = "STRAND_BRAID_GIT_HASH";

/// Set the environment variables `GIT_HASH` AND `CARGO_PKG_VERSION` to include
/// the current git revision.
///
/// The revision is read from `git rev-parse HEAD` when building inside a git
/// checkout. When building outside a git tree that command yields nothing, so
/// the revision must be supplied explicitly via the [`GIT_HASH_OVERRIDE_ENV`]
/// environment variable. If neither source provides one, the build fails
/// deliberately: an empty hash would produce the malformed version string
/// `"<version>+"` (invalid semver) that crashes consumers which parse the
/// version at runtime (issue #27), so we refuse to emit it rather than defer the
/// failure to startup.
pub fn git_hash(orig_version: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Rebuild if the override changes so the embedded hash stays in sync.
    println!("cargo:rerun-if-env-changed={GIT_HASH_OVERRIDE_ENV}");

    let git_hash = match head_rev() {
        Some(hash) => hash,
        None => override_from_env()?,
    };

    validate_build_metadata(&git_hash)?;

    println!("cargo:rustc-env=GIT_HASH={git_hash}");
    // Append the hash as semver build metadata; override cargo's default.
    println!("cargo:rustc-env=CARGO_PKG_VERSION={orig_version}+{git_hash}");
    Ok(())
}

/// The current `HEAD` commit hash, or `None` when it cannot be determined — git
/// absent, not a git checkout, or empty output. Returns `None` rather than
/// erroring so the caller can fall back to the environment override.
fn head_rev() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let hash = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!hash.is_empty()).then_some(hash)
}

/// The explicitly-provided revision for out-of-tree builds, or a build error
/// explaining how to supply one.
fn override_from_env() -> Result<String, Box<dyn std::error::Error>> {
    match std::env::var(GIT_HASH_OVERRIDE_ENV) {
        Ok(value) if !value.trim().is_empty() => Ok(value.trim().to_string()),
        _ => Err(format!(
            "could not determine the git revision: `git rev-parse HEAD` produced no \
             output (this is not a git checkout, or git is unavailable). Build from a \
             git clone, or set {GIT_HASH_OVERRIDE_ENV} to the commit hash you are \
             building from."
        )
        .into()),
    }
}

/// Ensure `hash` is a valid semver build-metadata value, so appending it to the
/// version can never produce an unparseable string that panics at runtime. The
/// grammar is dot-separated identifiers, each non-empty and made up only of
/// ASCII alphanumerics and hyphens.
fn validate_build_metadata(hash: &str) -> Result<(), Box<dyn std::error::Error>> {
    let valid = !hash.is_empty()
        && hash.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    if valid {
        Ok(())
    } else {
        Err(format!(
            "git revision {hash:?} is not a valid semver build-metadata identifier \
             (dot-separated ASCII alphanumeric/hyphen segments); refusing to embed it. \
             Check the {GIT_HASH_OVERRIDE_ENV} value."
        )
        .into())
    }
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
///   shared `trunk-target` directory beside the profile directory and forcing
///   the nested cargo offline (`CARGO_NET_OFFLINE=true`) to avoid deadlocking
///   the outer workspace cargo build on the target-dir and package-cache locks.
///   Trunk first runs `cargo metadata`, which resolves the whole workspace
///   graph for every platform, so the entire dependency graph (not just the
///   wasm32 subset) must already be in the cargo cache; on a cold cache,
///   pre-fetch it once with `cargo fetch`.
/// - Verifies each required asset is present in the dist directory.
/// - Emits `cargo:rerun-if-changed` directives for the frontend sources,
///   `index.html`, `Trunk.toml`, `scss/`, the calling `build.rs`, and every
///   in-tree Rust source that actually went into the wasm build (read back from
///   the nested cargo's dependency-info files).
pub fn trunk_build(
    frontend_dir: &str,
    required_assets: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::ErrorKind;
    use std::path::PathBuf;
    use std::process::Command;

    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    // Avoid deadlocking with the outer workspace cargo build by using a separate
    // target directory for trunk's nested cargo invocation.
    //
    // Keep that directory beside the profile directory rather than inside
    // OUT_DIR. OUT_DIR is keyed by a build-script hash that moves whenever the
    // *caller's* features, profile or dependencies change — none of which the
    // frontend depends on — so an OUT_DIR-local target directory rebuilt the
    // identical wasm from scratch several times a day and left a stale ~350 MB
    // copy behind each time. Both frontends share this one directory, which is
    // safe (trunk invocations are serialized by TrunkBuildLock, and cargo locks
    // the target directory itself) and lets them share compiled dependencies.
    let trunk_target_dir = shared_trunk_target_dir(&out_dir)?;
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
             cargo cache does not yet contain all dependencies. The nested `cargo \
             metadata` resolves the whole workspace for every platform, so the full \
             dependency graph must be cached. Pre-fetch it once with network access, \
             then rebuild:\n    \
             cargo fetch"
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

    // Re-run only when frontend sources or build script change. A
    // rerun-if-changed path that doesn't exist makes cargo treat the build
    // script as perpetually out of date (it has no mtime to compare against),
    // so every one of these must be gated on actually existing. Trunk.toml in
    // particular is optional (trunk works fine with its defaults), so not all
    // callers have one.
    for rel in [
        format!("{frontend_dir}/src"),
        format!("{frontend_dir}/scss"),
        format!("{frontend_dir}/index.html"),
        format!("{frontend_dir}/Trunk.toml"),
    ] {
        if PathBuf::from(&rel).exists() {
            println!("cargo:rerun-if-changed={rel}");
        }
    }
    println!("cargo:rerun-if-changed=build.rs");

    // The frontend also compiles workspace crates that live outside
    // `frontend_dir` — braid-types, strand-cam-types, ads-webasm, braid-mvg and
    // so on. Cargo's own dependency graph cannot notice when those change,
    // because the *caller* often does not depend on them at all (braid-run has
    // no native dependency on ads-webasm), and even when it does, rebuilding
    // the caller does not re-run its build script. Without the directives
    // below, editing a shared crate silently leaves a stale `dist/` embedded in
    // the binary — including a frontend that disagrees with the backend about
    // the wire types in braid-types. The nested cargo has just recorded exactly
    // which sources it read, so read them back rather than maintaining a list
    // here that would drift out of date the same way.
    // Dependency-info paths are absolute, built by the nested cargo from the
    // working directory this build script gave it, so anchor the frontend root
    // the same way rather than canonicalizing (which would resolve symlinks the
    // nested cargo did not).
    let frontend_root = normalize_lexically(
        &PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?).join(frontend_dir),
    );
    emit_rerun_for_wasm_sources(&trunk_target_dir, &frontend_root);

    Ok(())
}

/// The target directory for trunk's nested cargo invocation: `trunk-target`
/// beside the profile directory, i.e. `target/trunk-target`, or
/// `target/<triple>/trunk-target` when the outer build is cross-compiling.
///
/// `OUT_DIR` is `<base>/<profile>/build/<pkg>-<hash>/out`, so `<base>` is its
/// fifth ancestor.
fn shared_trunk_target_dir(
    out_dir: &std::path::Path,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let base = out_dir.ancestors().nth(4).ok_or_else(|| {
        format!(
            "cannot derive the target directory from OUT_DIR {}",
            out_dir.display()
        )
    })?;
    Ok(base.join("trunk-target"))
}

/// Emit a `cargo:rerun-if-changed` directive for every in-tree source that the
/// nested wasm build read, as recorded in the dependency-info (`.d`) files that
/// cargo writes next to the wasm artifacts.
///
/// Sources under the cargo home (registry and git checkouts) and under the
/// nested target directory itself are skipped: they are immutable or generated,
/// and pointing cargo at a file that later disappears would make the build
/// script perpetually out of date.
///
/// Both frontends share `trunk_target_dir`, so it holds a dependency-info file
/// per frontend. Only the ones that actually name a source inside
/// `frontend_root` describe *this* frontend; without that filter, braid-run
/// would rebuild its frontend whenever a strand-cam-only frontend source
/// changed.
fn emit_rerun_for_wasm_sources(
    trunk_target_dir: &std::path::Path,
    frontend_root: &std::path::Path,
) {
    let artifact_dir = trunk_target_dir
        .join("wasm32-unknown-unknown")
        .join("release");

    let entries = match std::fs::read_dir(&artifact_dir) {
        Ok(entries) => entries,
        Err(err) => {
            println!(
                "cargo:warning=Cannot watch the frontend's shared sources: {} is unreadable ({err}). \
                 Changes to crates outside the frontend directory may not trigger a rebuild.",
                artifact_dir.display()
            );
            return;
        }
    };

    let skip_prefixes: Vec<std::path::PathBuf> = cargo_home()
        .into_iter()
        .chain(std::iter::once(trunk_target_dir.to_path_buf()))
        .collect();

    let mut sources = std::collections::BTreeSet::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "d") {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let listed: Vec<std::path::PathBuf> = dep_info_sources(&contents)
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect();
        if !describes_frontend(&listed, frontend_root) {
            continue;
        }
        for source in listed {
            if skip_prefixes
                .iter()
                .any(|prefix| source.starts_with(prefix))
            {
                continue;
            }
            if source.is_file() {
                sources.insert(source);
            }
        }
    }

    if sources.is_empty() {
        println!(
            "cargo:warning=Found no dependency-info for the frontend build in {}. \
             Changes to crates outside the frontend directory may not trigger a rebuild.",
            artifact_dir.display()
        );
        return;
    }

    for source in sources {
        println!("cargo:rerun-if-changed={}", source.display());
    }
}

/// Resolve `.` and `..` in `path` without consulting the filesystem.
///
/// `frontend_dir` is relative to the caller's crate, and callers whose frontend
/// is a sibling reach sideways with `..` (flo builds `../flo-bui`). Joining that
/// onto `CARGO_MANIFEST_DIR` leaves a `..` in the middle of the path, and
/// `Path::starts_with` compares components literally, so the unresolved form
/// prefix-matches nothing in the dependency-info files.
///
/// Resolving lexically rather than with `canonicalize` deliberately preserves
/// any symlinked prefix: the nested cargo recorded its paths through whatever
/// prefix the outer build used, and rewriting one side but not the other would
/// reintroduce the same mismatch.
fn normalize_lexically(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;

    let mut out = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                // Only a real directory name can be popped.
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                // There is nothing above the root, so `/..` is just `/`.
                Some(Component::RootDir) => {}
                // A leading `..` in a relative path has nothing to resolve
                // against, so it has to be kept.
                _ => out.push(component),
            },
            other => out.push(other),
        }
    }
    out
}

/// Whether a dependency-info file belongs to the frontend rooted at
/// `frontend_root`, i.e. whether the wasm binary it describes was compiled from
/// at least one source inside that directory.
fn describes_frontend(sources: &[std::path::PathBuf], frontend_root: &std::path::Path) -> bool {
    sources
        .iter()
        .any(|source| source.starts_with(frontend_root))
}

/// The dependency paths listed in a makefile-style dependency-info file. Lines
/// have the form `<target>: <dep> <dep> ...`, so tokens ending in `:` are
/// targets rather than dependencies. Matching on a trailing colon (rather than
/// splitting on the first one) keeps Windows paths such as `C:\src\main.rs`
/// intact.
fn dep_info_sources(contents: &str) -> Vec<String> {
    contents
        .lines()
        .flat_map(dep_info_tokens)
        .filter(|token| !token.ends_with(':'))
        .collect()
}

/// Split one dependency-info line into paths. Following GNU make, only a
/// backslash immediately before a space escapes it; a backslash before anything
/// else is literal, which is what keeps Windows path separators intact.
fn dep_info_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&' ') => {
                chars.next();
                current.push(' ');
            }
            c if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// The cargo home directory, used only to recognise (and skip) immutable
/// registry and git-checkout sources. `None` when it cannot be determined, in
/// which case those sources are watched too — wasteful, but not wrong.
fn cargo_home() -> Option<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("CARGO_HOME") {
        return Some(std::path::PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(std::path::PathBuf::from(home).join(".cargo"))
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
/// It is implemented with a lock file created atomically via `create_new`;
/// the holder records its PID in the file. The lock is released when the
/// guard is dropped (which removes the file).
///
/// Holders regularly die without dropping the guard: rust-analyzer kills the
/// build scripts of an in-flight `cargo check` whenever a file save cancels
/// it, and SIGKILL runs no destructors. Waiters therefore treat the lock as
/// abandoned as soon as the recorded holder process no longer exists (checked
/// via `/proc`; on platforms without `/proc`, or for a lock file without a
/// readable PID, a file older than [`Self::STALE_AFTER`] is used as the
/// fallback rule) and steal it.
struct TrunkBuildLock {
    path: std::path::PathBuf,
}

impl TrunkBuildLock {
    /// Fallback when holder liveness cannot be determined: a lock file older
    /// than this is treated as abandoned by a crashed process. It is
    /// generous: it only needs to exceed the longest plausible trunk build,
    /// never a normal wait.
    const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(30 * 60);
    /// Give up rather than block a build forever if something is wrong.
    const ACQUIRE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

    fn acquire() -> Result<Self, Box<dyn std::error::Error>> {
        // A fixed, well-known path so every trunk build script on this machine
        // contends on the same lock.
        Self::acquire_at(std::env::temp_dir().join("strand-braid-trunk-build.lock"))
    }

    fn acquire_at(path: std::path::PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        use std::io::{ErrorKind, Write};

        let start = std::time::Instant::now();

        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    // Record our PID so waiters can detect a holder that died
                    // without cleaning up (see `is_stale`). If this write
                    // fails, waiters simply fall back to the age-based rule.
                    let _ = write!(file, "{}", std::process::id());
                    return Ok(Self { path });
                }
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    // Someone else holds the lock. Steal it if it is stale,
                    // otherwise wait and retry.
                    if Self::is_stale(&path) {
                        println!(
                            "cargo:warning=removing stale trunk build lock at {} \
                             (holder is gone)",
                            path.display()
                        );
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
        // Preferred rule: the holder recorded its PID; the lock is stale
        // exactly when that process no longer exists. (A recycled PID makes
        // the lock look held; the holder's death then ends the wait, and
        // ACQUIRE_TIMEOUT bounds the pathological case.)
        if let Ok(contents) = std::fs::read_to_string(path)
            && let Ok(pid) = contents.trim().parse::<u32>()
            && let Some(alive) = pid_is_alive(pid)
        {
            return !alive;
        }

        // Fallback rule (no readable PID — a mid-write race or a pre-PID
        // lock file — or no way to probe liveness on this platform): age.
        match std::fs::metadata(path).and_then(|meta| meta.modified()) {
            Ok(modified) => modified.elapsed().unwrap_or_default() > Self::STALE_AFTER,
            // If the file vanished between our open attempt and this check, it is
            // no longer held; treat it as stealable so we retry immediately.
            Err(_) => true,
        }
    }
}

/// Best-effort check whether a process with the given PID exists, without any
/// dependencies: on systems with `/proc` (Linux), a live process has an
/// entry there. Returns `None` where liveness cannot be determined (e.g.
/// macOS, Windows), in which case the caller falls back to an age-based rule.
fn pid_is_alive(pid: u32) -> Option<bool> {
    if std::path::Path::new("/proc/self").exists() {
        Some(std::path::Path::new(&format!("/proc/{pid}")).exists())
    } else {
        None
    }
}

impl Drop for TrunkBuildLock {
    fn drop(&mut self) {
        // Only remove the lock if it is still ours: if it was deemed stale,
        // stolen, and re-acquired by another process, removing it here would
        // release that process's lock out from under it.
        let is_ours = std::fs::read_to_string(&self.path)
            .map(|contents| contents.trim() == std::process::id().to_string())
            .unwrap_or(false);
        if is_ours {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        dep_info_sources, describes_frontend, normalize_lexically, shared_trunk_target_dir,
        validate_build_metadata,
    };

    #[test]
    fn resolves_a_sideways_frontend_dir() {
        // flo builds `../flo-bui` from crates/flo-webserver.
        assert_eq!(
            normalize_lexically(std::path::Path::new("/flo/crates/flo-webserver/../flo-bui")),
            std::path::Path::new("/flo/crates/flo-bui")
        );
        assert_eq!(
            normalize_lexically(std::path::Path::new("/a/./b/")),
            std::path::Path::new("/a/b")
        );
    }

    #[test]
    fn keeps_parent_components_it_cannot_resolve() {
        assert_eq!(
            normalize_lexically(std::path::Path::new("../a/b")),
            std::path::Path::new("../a/b")
        );
        assert_eq!(
            normalize_lexically(std::path::Path::new("/../a")),
            std::path::Path::new("/a")
        );
    }

    #[test]
    fn a_sideways_frontend_root_matches_its_dep_info() {
        let sources = [std::path::PathBuf::from("/flo/crates/flo-bui/src/main.rs")];
        let unresolved = std::path::Path::new("/flo/crates/flo-webserver/../flo-bui");
        // The unresolved form is what silently matched nothing.
        assert!(!describes_frontend(&sources, unresolved));
        assert!(describes_frontend(
            &sources,
            &normalize_lexically(unresolved)
        ));
    }

    #[test]
    fn tells_the_two_frontends_dep_info_apart() {
        let braid = [
            std::path::PathBuf::from("/w/braid/braid-run/braid_frontend/src/main.rs"),
            std::path::PathBuf::from("/w/braid/braid-types/src/lib.rs"),
        ];
        let braid_root = std::path::Path::new("/w/braid/braid-run/braid_frontend");
        let strand_cam_root = std::path::Path::new("/w/strand-cam/yew_frontend");
        assert!(describes_frontend(&braid, braid_root));
        assert!(!describes_frontend(&braid, strand_cam_root));
    }

    #[test]
    fn derives_a_shared_trunk_target_dir() {
        let out_dir = std::path::Path::new("target/release/build/pkg-hash/out");
        assert_eq!(
            shared_trunk_target_dir(out_dir).unwrap(),
            std::path::Path::new("target/trunk-target")
        );
    }

    #[test]
    fn keeps_the_trunk_target_dir_per_cross_compilation_triple() {
        let out_dir = std::path::Path::new("target/wasm32-unknown-unknown/release/build/p-h/out");
        assert_eq!(
            shared_trunk_target_dir(out_dir).unwrap(),
            std::path::Path::new("target/wasm32-unknown-unknown/trunk-target")
        );
    }

    #[test]
    fn reads_dependencies_but_not_the_target_from_dep_info() {
        let contents = "/w/target/f.wasm: /w/src/main.rs /w/braid-types/src/lib.rs\n";
        assert_eq!(
            dep_info_sources(contents),
            ["/w/src/main.rs", "/w/braid-types/src/lib.rs"]
        );
    }

    #[test]
    fn unescapes_spaces_but_keeps_windows_separators() {
        let contents = r"C:\w\f.wasm: C:\My\ Code\src\main.rs";
        assert_eq!(dep_info_sources(contents), [r"C:\My Code\src\main.rs"]);
    }

    #[test]
    fn ignores_phony_target_lines() {
        let contents = "/w/f.wasm: /w/src/main.rs\n/w/src/main.rs:\n";
        assert_eq!(dep_info_sources(contents), ["/w/src/main.rs"]);
    }

    #[test]
    fn accepts_a_git_hash() {
        assert!(validate_build_metadata("8581679a9cf313a24b230081637ffd4fc27568ad").is_ok());
    }

    #[test]
    fn accepts_dotted_and_hyphenated_identifiers() {
        // e.g. an override such as a `git describe` output.
        assert!(validate_build_metadata("1.0.0-rc.3-5-gabc123").is_ok());
    }

    #[test]
    fn rejects_empty() {
        assert!(validate_build_metadata("").is_err());
    }

    #[test]
    fn rejects_empty_segment() {
        // A trailing dot leaves an empty segment — the class of malformed value
        // (like the bare trailing `+`) that crashed consumers at runtime.
        assert!(validate_build_metadata("abc.").is_err());
        assert!(validate_build_metadata(".abc").is_err());
    }

    #[test]
    fn rejects_invalid_characters() {
        assert!(validate_build_metadata("has space").is_err());
        assert!(validate_build_metadata("plus+sign").is_err());
        assert!(validate_build_metadata("under_score").is_err());
    }

    mod trunk_build_lock {
        use super::super::TrunkBuildLock;

        /// A unique lock path per test so tests neither collide with each
        /// other nor with the real machine-wide lock.
        fn test_lock_path(name: &str) -> std::path::PathBuf {
            std::env::temp_dir().join(format!(
                "build-util-test-{name}-{}.lock",
                std::process::id()
            ))
        }

        /// A PID that is certain not to exist: spawn a short-lived process
        /// and reap it. (The PID could in principle be recycled before the
        /// test reads it, but the window is microseconds.)
        #[cfg(target_os = "linux")]
        fn dead_pid() -> u32 {
            let mut child = std::process::Command::new("true").spawn().unwrap();
            let pid = child.id();
            child.wait().unwrap();
            pid
        }

        #[test]
        fn acquire_records_pid_and_drop_removes() {
            let path = test_lock_path("acquire");
            let lock = TrunkBuildLock::acquire_at(path.clone()).unwrap();
            let contents = std::fs::read_to_string(&path).unwrap();
            assert_eq!(contents, std::process::id().to_string());
            drop(lock);
            assert!(!path.exists(), "drop must remove the lock file");
        }

        #[test]
        fn drop_leaves_a_lock_that_is_not_ours() {
            let path = test_lock_path("not-ours");
            let lock = TrunkBuildLock::acquire_at(path.clone()).unwrap();
            // Simulate the lock having been stolen and re-acquired by
            // another process.
            std::fs::write(&path, "0").unwrap();
            drop(lock);
            assert!(path.exists(), "drop must not remove another holder's lock");
            std::fs::remove_file(&path).unwrap();
        }

        #[cfg(target_os = "linux")]
        #[test]
        fn lock_of_live_holder_is_not_stale() {
            let path = test_lock_path("live");
            std::fs::write(&path, std::process::id().to_string()).unwrap();
            assert!(!TrunkBuildLock::is_stale(&path));
            std::fs::remove_file(&path).unwrap();
        }

        #[cfg(target_os = "linux")]
        #[test]
        fn lock_of_dead_holder_is_stale_and_stolen() {
            let path = test_lock_path("dead");
            std::fs::write(&path, dead_pid().to_string()).unwrap();
            assert!(TrunkBuildLock::is_stale(&path));

            // A fresh acquire must steal it promptly (well under the 30 min
            // age rule) and record its own PID.
            let lock = TrunkBuildLock::acquire_at(path.clone()).unwrap();
            let contents = std::fs::read_to_string(&path).unwrap();
            assert_eq!(contents, std::process::id().to_string());
            drop(lock);
        }

        #[test]
        fn unreadable_pid_falls_back_to_age_rule() {
            let path = test_lock_path("no-pid");
            // An empty, freshly-created lock file (e.g. the moment between
            // create_new and the PID write): recent mtime, so held.
            std::fs::write(&path, "").unwrap();
            assert!(!TrunkBuildLock::is_stale(&path));
            std::fs::remove_file(&path).unwrap();
        }
    }
}
