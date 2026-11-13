// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use clap::Parser;

use braidz_cli::Opt;

fn main() -> anyhow::Result<()> {
    env_tracing_logger::init();
    let opt = Opt::parse();
    let stdout = std::io::stdout();
    braidz_cli::run(&opt, &chrono::Local, &mut stdout.lock())
}
