// Copyright 2020-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <INPUT> <OUTPUT.zip>", args[0]);
        anyhow::bail!("No <INPUT> or <OUTPUT.zip> filename given");
    }
    let input_dirname = std::path::PathBuf::from(&args[1]);
    let output_fname = std::path::PathBuf::from(&args[2]);

    println!("-> {}", output_fname.display());
    zip_or_dir::copy_to_zip(input_dirname, output_fname)?;

    Ok(())
}
