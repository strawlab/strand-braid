#[cfg(any(feature = "bundle_files", feature = "serve_files"))]
use std::error::Error;
#[cfg(any(feature = "bundle_files", feature = "serve_files"))]
use std::path::Path;

use std::process::Command;

fn git_hash() {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git");
    let git_hash = String::from_utf8(output.stdout).expect("from_utf8");
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
}

/// Do codegen to write a file (`codegen_fname`) which includes
/// the contents of all entries in `files_dir`.
#[cfg(feature = "bundle_files")]
fn create_codegen_file<P, Q>(files_dir: P, codegen_fname: Q) -> Result<(), std::io::Error>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    // Collect list of files to include
    let entries = walkdir::WalkDir::new(files_dir.as_ref())
        .into_iter()
        .map(|entry| entry.expect("DirEntry error").path().into())
        .collect::<Vec<std::path::PathBuf>>();

    // Make sure we recompile if these files change
    println!("cargo:rerun-if-changed={}", files_dir.as_ref().display());
    for entry in entries.iter() {
        println!("cargo:rerun-if-changed={}", entry.display());
    }

    // Check that at least one of the needed files is there.
    let required: std::path::PathBuf = files_dir.as_ref().join("index.html");
    if !entries.contains(&required) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("no {:?} file (hint: run make in elm_frontend)", required),
        ));
    }

    let codegen_fname_str = format!("{}", codegen_fname.as_ref().display());
    // Write the contents of the files.
    includedir_codegen::start("PUBLIC")
        .dir(files_dir, includedir_codegen::Compression::Gzip)
        .build(&codegen_fname_str)?;
    Ok(())
}

/// Create an empty file (`codegen_fname`).
#[cfg(feature = "serve_files")]
fn create_codegen_file<P, Q>(_: P, codegen_fname: Q) -> Result<(), Box<dyn Error>>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let out_dir = std::env::var("OUT_DIR")?;
    let dest_path = std::path::Path::new(&out_dir).join(codegen_fname);
    std::fs::File::create(dest_path)?;
    Ok(())
}

#[cfg(any(feature = "bundle_files", feature = "serve_files"))]
pub fn codegen<P, Q>(files_dir: P, generated_path: Q) -> Result<(), Box<dyn Error>>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    create_codegen_file(&files_dir, &generated_path)?;
    Ok(())
}

#[cfg(not(any(feature = "bundle_files", feature = "serve_files")))]
compile_error!("Need cargo feature \"bundle_files\" or \"serve_files\"");

fn main() {
    #[cfg(any(feature = "bundle_files", feature = "serve_files"))]
    codegen("static", "public.rs").expect("codegen failed");
    git_hash();
}
