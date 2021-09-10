// modified from https://github.com/mvdnes/zip-rs/blob/master/examples/write_dir.rs

use std::io::{Seek, Write};
use std::iter::Iterator;
use zip::{result::ZipResult, write::FileOptions, ZipWriter};

use std::fs::File;
use std::path::Path;

pub(crate) fn zip_dir<T, P>(
    it: &mut dyn Iterator<Item = walkdir::DirEntry>,
    prefix: P,
    mut zipw: ZipWriter<T>,
    options: FileOptions,
) -> ZipResult<()>
where
    T: Write + Seek,
    P: AsRef<Path>,
{
    for entry in it {
        let path = entry.path();
        let name = path.strip_prefix(prefix.as_ref()).unwrap();
        let name_string = name.to_str().unwrap().to_string();

        // Write file or directory explicitly
        // Some unzip tools unzip files with directory paths correctly, some do not!
        if path.is_file() {
            zipw.start_file(name_string, options)?;
            let mut f = File::open(path)?;
            std::io::copy(&mut f, &mut zipw)?;
        } else if !name.as_os_str().is_empty() {
            // Only if not root! Avoids path spec / warning
            // and mapname conversion failed error on unzip
            zipw.add_directory(name_string, options)?; // Discussion about deprecation error at https://github.com/zip-rs/zip/issues/181
        }
    }
    zipw.finish()?;
    Result::Ok(())
}
