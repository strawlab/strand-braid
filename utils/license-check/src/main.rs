//! CLI program to check (or add) SPDX license headers on the `.rs` files of a
//! rust workspace.
//!
//! Every tracked `*.rs` file must declare an `SPDX-License-Identifier:` line in
//! its header. By default the required expression is `MIT OR Apache-2.0`. Paths
//! listed in `allowlist.toml` (compiled into this binary) require a different
//! expression instead, because they originate from third-party code under
//! another license.
//!
//! Usage to check all files (this is what CI runs):
//! ```bash
//!   license-check
//! ```
//!
//! Usage to add missing headers in place:
//! ```bash
//!   license-check --fix
//! ```
use camino::{Utf8Path, Utf8PathBuf};
use clap::Parser;
use eyre::{Result, WrapErr};
use serde::Deserialize;

/// The SPDX expression required of every file not covered by the allowlist.
const DEFAULT_SPDX: &str = "MIT OR Apache-2.0";

/// The copyright line written by `--fix` when adding a new header.
const COPYRIGHT_LINE: &str = "// Copyright (C) The Strand-Braid Authors";

/// Number of leading lines scanned for the SPDX identifier and for an existing
/// header. License headers always appear at the very top of a file.
const HEADER_SCAN_LINES: usize = 15;

/// The allowlist, compiled into the binary so the check is hermetic.
const ALLOWLIST_TOML: &str = include_str!("../allowlist.toml");

#[derive(Parser)]
#[command(about = "Check (or add) SPDX license headers on workspace .rs files")]
struct Cli {
    /// Add a header to files that are missing one, in place.
    ///
    /// Without this flag the program only checks and reports, exiting non-zero
    /// if any file is non-compliant.
    #[arg(long)]
    fix: bool,

    /// The root of the workspace. If not given the current directory is used.
    workspace_root: Option<Utf8PathBuf>,
}

#[derive(Deserialize)]
struct Allowlist {
    #[serde(default)]
    allow: Vec<AllowEntry>,
}

#[derive(Deserialize)]
struct AllowEntry {
    /// Literal path-prefix (repo-relative) this entry applies to.
    prefix: String,
    /// SPDX expression required for matching paths.
    spdx: String,
}

impl Allowlist {
    /// The SPDX expression required for `path`, honoring allowlist entries.
    fn required_spdx(&self, path: &str) -> &str {
        self.allow
            .iter()
            .find(|entry| path.starts_with(&entry.prefix))
            .map(|entry| entry.spdx.as_str())
            .unwrap_or(DEFAULT_SPDX)
    }
}

/// Outcome of inspecting a single file.
enum Status {
    /// File declares the expected SPDX identifier.
    Ok,
    /// No `SPDX-License-Identifier:` line in the header.
    Missing,
    /// Declares an SPDX identifier that differs from the expected one.
    Mismatch { found: String, expected: String },
}

/// Extract the value of the `SPDX-License-Identifier:` line in the file header,
/// if present.
fn extract_spdx(content: &str) -> Option<String> {
    content.lines().take(HEADER_SCAN_LINES).find_map(|line| {
        line.split_once("SPDX-License-Identifier:")
            .map(|(_, rest)| rest.trim().to_string())
    })
}

fn status_of(content: &str, expected: &str) -> Status {
    match extract_spdx(content) {
        None => Status::Missing,
        Some(found) if found == expected => Status::Ok,
        Some(found) => Status::Mismatch {
            found,
            expected: expected.to_string(),
        },
    }
}

/// Whether `line` is a leading "boilerplate" comment line (`//` but not a `//!`
/// or `///` doc comment).
fn is_plain_comment(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") && !t.starts_with("//!") && !t.starts_with("///")
}

/// Whether `line` is the start of the historical in-house license header
/// (`// Copyright <years> Andrew D. Straw.`).
fn is_inhouse_copyright(line: &str) -> bool {
    line.starts_with("// Copyright") && line.contains("Andrew D. Straw")
}

/// Remove an existing in-house license header of the historical form
/// (`// Copyright <years> Andrew D. Straw.` followed by the dual-license prose
/// block) from `content`, returning the remainder. The header may appear at the
/// very top of the file or just below a leading `//!` module-doc block. Returns
/// `None` if no such in-house header is present.
fn remove_inhouse_header(content: &str) -> Option<String> {
    let mut lines: Vec<&str> = content.lines().collect();
    let start = lines
        .iter()
        .take(HEADER_SCAN_LINES)
        .position(|l| is_inhouse_copyright(l))?;
    // The header is the contiguous run of plain comment lines from `start`.
    let mut end = start;
    while end < lines.len() && is_plain_comment(lines[end]) {
        end += 1;
    }
    lines.drain(start..end);
    // Collapse a blank line left on each side of the removed block into one,
    // and drop a now-leading blank line.
    if start > 0
        && start < lines.len()
        && lines[start - 1].trim().is_empty()
        && lines[start].trim().is_empty()
    {
        lines.remove(start);
    }
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    let mut out = lines.join("\n");
    if content.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    Some(out)
}

/// Whether the file already begins with some copyright/license comment that is
/// NOT our historical in-house header (i.e. third-party attribution we must not
/// rewrite automatically).
fn has_foreign_header(content: &str) -> bool {
    content
        .lines()
        .take(HEADER_SCAN_LINES)
        .any(|l| l.contains("Copyright") || l.contains("License"))
}

/// Build the two-line header for the given SPDX expression.
fn new_header(spdx: &str) -> String {
    format!("{COPYRIGHT_LINE}\n// SPDX-License-Identifier: {spdx}\n")
}

/// Compute the new file contents for `--fix`, or `None` if the file cannot be
/// fixed automatically and needs manual attention.
fn fixed_content(content: &str, expected: &str) -> Option<String> {
    let header = new_header(expected);
    if let Some(rest) = remove_inhouse_header(content) {
        // Replace the historical header with the new one, placed at the top.
        Some(format!("{header}\n{rest}"))
    } else if has_foreign_header(content) {
        // Third-party attribution: don't touch it automatically.
        None
    } else {
        // No header at all: prepend.
        Some(format!("{header}\n{content}"))
    }
}

/// Return the repo-relative paths of all tracked `*.rs` files.
fn tracked_rs_files(root: &Utf8Path) -> Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .args(["ls-files", "*.rs"])
        .current_dir(root)
        .output()
        .wrap_err("failed to run `git ls-files`")?;
    if !output.status.success() {
        eyre::bail!(
            "`git ls-files` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8(output.stdout)
        .wrap_err("`git ls-files` produced non-UTF-8 output")?
        .lines()
        .map(|s| s.to_string())
        .collect())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = cli.workspace_root.unwrap_or_else(|| Utf8PathBuf::from("."));

    let allowlist: Allowlist =
        toml::from_str(ALLOWLIST_TOML).wrap_err("failed to parse embedded allowlist.toml")?;

    let files = tracked_rs_files(&root)?;

    let mut missing = Vec::new();
    let mut mismatched = Vec::new();
    let mut fixed = Vec::new();
    let mut manual = Vec::new();

    for rel in &files {
        let path = root.join(rel);
        let content =
            std::fs::read_to_string(&path).wrap_err_with(|| format!("failed to read {path}"))?;
        let expected = allowlist.required_spdx(rel);

        match status_of(&content, expected) {
            Status::Ok => {}
            Status::Mismatch { found, expected } => {
                mismatched.push((rel.clone(), found, expected));
            }
            Status::Missing => {
                if cli.fix {
                    match fixed_content(&content, expected) {
                        Some(new) => {
                            std::fs::write(&path, new)
                                .wrap_err_with(|| format!("failed to write {path}"))?;
                            fixed.push(rel.clone());
                        }
                        None => manual.push(rel.clone()),
                    }
                } else {
                    missing.push(rel.clone());
                }
            }
        }
    }

    for rel in &fixed {
        println!("fixed:    {rel}");
    }
    for rel in &missing {
        println!("MISSING:  {rel}");
    }
    for (rel, found, expected) in &mismatched {
        println!("MISMATCH: {rel} (found `{found}`, expected `{expected}`)");
    }
    for rel in &manual {
        println!("MANUAL:   {rel} (third-party header; add SPDX line by hand)");
    }

    let checked = files.len();
    let bad = missing.len() + mismatched.len() + manual.len();
    if bad == 0 {
        println!("license-check: all {checked} .rs files OK");
        Ok(())
    } else {
        eprintln!(
            "license-check: {bad} of {checked} .rs files need attention \
             (run `cargo run -p license-check -- --fix` to add missing in-house headers)"
        );
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_spdx() {
        let c = "// Copyright (C) The Strand-Braid Authors\n// SPDX-License-Identifier: MIT OR Apache-2.0\n\nfn main() {}\n";
        assert_eq!(extract_spdx(c).as_deref(), Some("MIT OR Apache-2.0"));
    }

    #[test]
    fn missing_spdx() {
        assert_eq!(extract_spdx("fn main() {}\n"), None);
    }

    #[test]
    fn allowlist_prefix() {
        let list = Allowlist {
            allow: vec![AllowEntry {
                prefix: "a/b/".to_string(),
                spdx: "BSD-2-Clause".to_string(),
            }],
        };
        assert_eq!(list.required_spdx("a/b/c.rs"), "BSD-2-Clause");
        assert_eq!(list.required_spdx("a/x.rs"), DEFAULT_SPDX);
    }

    #[test]
    fn embedded_allowlist_parses() {
        let _: Allowlist = toml::from_str(ALLOWLIST_TOML).unwrap();
    }

    #[test]
    fn fix_prepends_when_no_header() {
        let c = "fn main() {}\n";
        let out = fixed_content(c, DEFAULT_SPDX).unwrap();
        assert!(out.starts_with(COPYRIGHT_LINE));
        assert!(out.contains("SPDX-License-Identifier: MIT OR Apache-2.0"));
        assert!(out.ends_with("fn main() {}\n"));
    }

    #[test]
    fn fix_replaces_old_inhouse_header() {
        let c = "// Copyright 2020-2023 Andrew D. Straw.\n//\n// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or\n// copied, modified, or distributed except according to those terms.\n\n//! Module docs.\nfn main() {}\n";
        let out = fixed_content(c, DEFAULT_SPDX).unwrap();
        assert!(out.starts_with(COPYRIGHT_LINE));
        assert!(!out.contains("Andrew D. Straw"));
        assert!(out.contains("//! Module docs."));
        assert_eq!(out.matches("SPDX-License-Identifier").count(), 1);
    }

    #[test]
    fn fix_replaces_inhouse_header_below_module_doc() {
        let c = "//! Module docs.\n//! more docs.\n\n// Copyright 2020-2023 Andrew D. Straw.\n//\n// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or\n// copied, modified, or distributed except according to those terms.\n\nuse foo::bar;\n";
        let out = fixed_content(c, DEFAULT_SPDX).unwrap();
        assert!(out.starts_with(COPYRIGHT_LINE));
        assert!(!out.contains("Andrew D. Straw"));
        assert!(out.contains("//! Module docs."));
        assert!(out.contains("use foo::bar;"));
        assert_eq!(out.matches("SPDX-License-Identifier").count(), 1);
        // No doubled blank line where the header used to be.
        assert!(!out.contains("\n\n\n"));
    }

    #[test]
    fn fix_skips_foreign_header() {
        let c = "// Copyright 2017-2022 Brian Langenberger\n//\n// Licensed under the Apache License, Version 2.0\nfn main() {}\n";
        assert!(fixed_content(c, DEFAULT_SPDX).is_none());
    }
}
