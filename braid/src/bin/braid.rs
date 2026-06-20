// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeSet;

use clap::{CommandFactory, Parser};
use eyre::{Result, WrapErr};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct BraidLauncherCliArgs {
    /// Command to execute (e.g. run, show-config, default-config, help).
    ///
    /// `braid <command>` dispatches to the `braid-<command>` executable. Run
    /// `braid` with no command (or `braid help`) to list the available
    /// commands.
    command: Option<String>,
    /// Options specific to the command
    options: Vec<String>,
}

#[cfg(target_os = "windows")]
const EXE_SUFFIX: &str = ".exe";
#[cfg(not(target_os = "windows"))]
const EXE_SUFFIX: &str = "";

/// Whether `path` looks like an executable file we can dispatch to. On Unix this
/// checks the executable permission bit; on other platforms it is sufficient
/// that the entry is a regular file (the `.exe` suffix is already required by
/// the caller).
fn is_executable_file(path: &std::path::Path) -> bool {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return false,
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Discover the available `braid-<command>` subcommands by scanning the
/// directory containing the current executable and every directory on `PATH`
/// for executables named `braid-*`. Returns the bare command names (the part
/// after the `braid-` prefix), sorted and de-duplicated.
fn discover_subcommands() -> BTreeSet<String> {
    let prefix = "braid-";
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.to_path_buf());
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }

    let mut found = BTreeSet::new();
    for dir in dirs {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name = match file_name.to_str() {
                Some(name) => name,
                None => continue,
            };
            // On Windows this requires (and removes) the `.exe` suffix; on Unix
            // `EXE_SUFFIX` is empty so this is a no-op.
            let name = match name.strip_suffix(EXE_SUFFIX) {
                Some(name) => name,
                None => continue,
            };
            let sub = match name.strip_prefix(prefix) {
                Some(sub) if !sub.is_empty() => sub,
                _ => continue,
            };
            if is_executable_file(&entry.path()) {
                found.insert(sub.to_string());
            }
        }
    }
    found
}

/// Format the discovered subcommands as an indented, newline-separated list (or
/// a short note if none were found on this system).
fn format_subcommands(subcommands: &BTreeSet<String>) -> String {
    if subcommands.is_empty() {
        "    (no `braid-*` commands found on PATH)".to_string()
    } else {
        subcommands
            .iter()
            .map(|sub| format!("    braid {sub}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn main() -> Result<()> {
    env_tracing_logger::init();

    let args = BraidLauncherCliArgs::parse();
    tracing::debug!("{:?}", args);

    let command = match args.command.as_deref() {
        // No command, or an explicit help request: show the launcher help
        // followed by the list of available `braid-<command>` executables.
        None | Some("help") => {
            let help = BraidLauncherCliArgs::command().render_long_help();
            println!("{}", help.ansi());
            println!("Available commands:");
            println!("{}", format_subcommands(&discover_subcommands()));
            return Ok(());
        }
        Some(command) => command,
    };

    let cmd_name = format!("braid-{command}{EXE_SUFFIX}");

    let status = match std::process::Command::new(format!("braid-{command}"))
        .args(&args.options)
        .status()
    {
        Ok(status) => status,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            // The most common cause is a mistyped or unknown command. Help the
            // user by listing what is actually available.
            eprintln!("braid: unknown command '{command}' (no '{cmd_name}' executable found).\n");
            eprintln!("Available commands:");
            eprintln!("{}", format_subcommands(&discover_subcommands()));
            std::process::exit(2);
        }
        Err(err) => {
            return Err(err).with_context(|| format!("running '{cmd_name}'"));
        }
    };

    if let Some(code) = status.code() {
        std::process::exit(code);
    }

    tracing::debug!("done");

    Ok(())
}
