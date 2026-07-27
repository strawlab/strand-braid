// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::path::PathBuf;

use clap::Parser;
use download_verify::{Hash, download_verify};

/// Download a file from a URL and verify its SHA-256 hash, the same way this
/// workspace's own tests use the `download-verify` library.
#[derive(Parser)]
struct Cli {
    /// URL to download from.
    #[arg(long)]
    url: String,
    /// Expected SHA-256 hash of the file, as a hex string.
    #[arg(long)]
    sha256: String,
    /// Local path to save the file to. If it already exists, its hash is
    /// checked instead of re-downloading.
    #[arg(long)]
    dest: PathBuf,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = download_verify(&cli.url, &cli.dest, &Hash::Sha256(cli.sha256)) {
        eprintln!("download-verify: {e}");
        std::process::exit(1);
    }
}
