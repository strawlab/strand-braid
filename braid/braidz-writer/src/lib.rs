// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

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

/// Zip the `output_dirname` directory into the `.braidz` file `output_zipfile`.
///
/// The file is written beside `output_zipfile` and moved there only once it is
/// complete, so that an interrupted conversion never leaves a truncated
/// `.braidz` file. `output_dirname` is left untouched: it remains the copy to
/// fall back on.
pub fn dir_to_braidz<P1: AsRef<Path>, P2: AsRef<Path>>(
    output_dirname: P1,
    output_zipfile: P2,
) -> Result<(), Error> {
    let (mut file, pending) = pending_file::PendingFile::create(&output_zipfile)?;

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
        if entry.file_name() == braid_types::README_MD_FNAME {
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
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true)
        .unix_permissions(0o755);

    zip_dir::zip_dir(&mut files.into_iter(), &output_dirname, &mut zipw, options)?;
    // This returns the file, which is closed here.
    zipw.finish()?;
    pending.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names in `dir`, sorted.
    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// The `.braidz` file is written under a temporary name and moved into
    /// place. Afterwards there must be the source directory, the `.braidz`
    /// file, and nothing else; and an existing `.braidz` file is replaced.
    #[test]
    fn dir_to_braidz_leaves_only_the_braidz() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        let src = root.path().join("rec.braid");
        std::fs::create_dir(&src)?;
        std::fs::write(src.join(braid_types::README_MD_FNAME), "readme")?;
        std::fs::write(src.join("data.csv"), "a,b\n1,2\n")?;
        let dest = root.path().join("rec.braidz");
        std::fs::write(&dest, "stale")?;

        dir_to_braidz(&src, &dest)?;
        assert_eq!(names_in(root.path()), ["rec.braid", "rec.braidz"]);

        let mut archive = zip::ZipArchive::new(std::fs::File::open(&dest)?)?;
        assert_eq!(archive.len(), 2);
        assert!(archive.by_name("data.csv").is_ok());
        Ok(())
    }
}
