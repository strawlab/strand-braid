// modified from https://github.com/mvdnes/zip-rs/blob/master/examples/write_dir.rs

use std::io::{Seek, Write};
use std::iter::Iterator;
use zip::{result::ZipResult, write::FileOptions, ZipWriter};

use std::fs::File;
use std::path::Path;

pub(crate) fn zip_dir<T, P>(
    it: &mut dyn Iterator<Item = walkdir::DirEntry>,
    prefix: P,
    mut zipw: &mut ZipWriter<T>,
    options: FileOptions,
) -> ZipResult<()>
where
    T: Write + Seek,
    P: AsRef<Path>,
{
    for entry in it {
        let path = entry.path();
        let name = path.strip_prefix(prefix.as_ref()).unwrap();

        // Join path components with forward slash ("/") because this is how zip
        // files stores them. This is important because on Windows path
        // components are separated with back slash ("\").
        let name_string = name
            .components()
            .map(|c| c.as_os_str().to_str().unwrap())
            .collect::<Vec<&str>>()
            .join("/");

        // Write file or directory explicitly
        // Some unzip tools unzip files with directory paths correctly, some do not!
        if path.is_file() {
            zipw.start_file(name_string, options)?;
            let mut f = File::open(path)?;
            std::io::copy(&mut f, &mut zipw)?;
        } else if !name_string.is_empty() {
            // Only if not root! Avoids path spec / warning
            // and mapname conversion failed error on unzip
            zipw.add_directory(name_string, options)?; // Discussion about deprecation error at https://github.com/zip-rs/zip/issues/181
        }
    }
    Result::Ok(())
}

#[test]
fn test_nested_names() -> anyhow::Result<()> {
    let output_root = tempfile::tempdir().unwrap(); // will cleanup on drop
    let file1 = output_root.path().join("file1.txt");
    std::fs::write(file1, "file 1 contents")?;
    let subdir1 = output_root.path().join("subdir1");
    std::fs::create_dir_all(&subdir1)?;
    let file2 = subdir1.join("file2.txt");
    std::fs::write(file2, "file 2 contents")?;

    let mut zipw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let walkdir = walkdir::WalkDir::new(&output_root);
    let mut file_iter = walkdir.into_iter().map(|x| x.unwrap());
    zip_dir(
        &mut file_iter,
        output_root,
        &mut zipw,
        FileOptions::default(),
    )?;

    let buf = zipw.finish()?.into_inner();
    let zip_archive = zip::ZipArchive::new(std::io::Cursor::new(&buf[..]))?;
    let mut fnames: std::collections::BTreeSet<&str> = zip_archive.file_names().collect();
    assert!(fnames.remove("file1.txt"));
    assert!(fnames.remove("subdir1/"));
    assert!(fnames.remove("subdir1/file2.txt"));
    assert_eq!(fnames.len(), 0);
    Ok(())
}
