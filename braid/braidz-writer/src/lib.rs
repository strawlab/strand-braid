use std::{io::Write, path::Path};

mod zip_dir;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("IO error: {source}")]
    IoError {
        #[from]
        source: std::io::Error,
    },
    #[error("zip error: {source}")]
    ZipError {
        #[from]
        source: zip::result::ZipError,
    },
}

// zip the output_dirname directory
pub fn dir_to_braidz<P1: AsRef<Path>, P2: AsRef<Path>>(
    output_dirname: P1,
    output_zipfile: P2,
) -> Result<(), Error> {
    let mut file = std::fs::File::create(&output_zipfile)?;

    let header = "BRAIDZ file. This is a standard ZIP file with a \
                        specific schema. You can view the contents of this \
                        file at https://braidz.strawlab.org/\n";
    file.write_all(header.as_bytes())?;

    let walkdir = walkdir::WalkDir::new(&output_dirname);

    // Reorder the results to save the README_MD_FNAME file first
    // so that the first bytes of the file have it. This is why we
    // special-case the file here.
    let mut readme_entry: Option<walkdir::DirEntry> = None;

    let mut files = Vec::new();
    for entry in walkdir.into_iter().filter_map(|e| e.ok()) {
        if entry.file_name() == flydra_types::README_MD_FNAME {
            readme_entry = Some(entry);
        } else {
            files.push(entry);
        }
    }
    if let Some(entry) = readme_entry {
        files.insert(0, entry);
    }

    let mut zipw = zip::ZipWriter::new(file);
    // Since most of our files are already compressed as .gz files,
    // we do not bother attempting to compress again. This would
    // cost significant computation but wouldn't save much space.
    // (The compressed files should all end with .gz so we could
    // theoretically compress the uncompressed files by a simple
    // file name filter. However, the README.md file should ideally
    // remain uncompressed and as the first file so that inspecting
    // the braidz file will show this.)
    let options = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true)
        .unix_permissions(0o755);

    zip_dir::zip_dir(&mut files.into_iter(), &output_dirname, &mut zipw, options).expect("zip_dir");
    zipw.finish()?;
    Ok(())
}
