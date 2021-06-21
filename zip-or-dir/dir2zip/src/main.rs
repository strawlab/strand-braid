fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().map(String::from).collect();
    if args.len() != 3 {
        assert!(args.len() >= 1);
        eprintln!("Usage: {} <INPUT> <OUTPUT.zip>", args[0]);
        anyhow::bail!("No <INPUT> or <OUTPUT.zip> filename given");
    }
    let input_dirname = std::path::PathBuf::from(&args[1]);
    let output_fname = std::path::PathBuf::from(&args[2]);

    println!("-> {}", output_fname.display());
    zip_or_dir::copy_to_zip(input_dirname, output_fname)?;

    Ok(())
}
