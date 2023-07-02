// Copyright 2020-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

#![cfg_attr(
    feature = "backtrace",
    feature(error_generic_member_access, provide_any)
)]

//! An archive of "files", either in a filesystem directory or zip archive.
//!
//! The primary object of interest in this crate is the struct
//! [ZipDirArchive](struct.ZipDirArchive.html), which provides a read-only
//! wrapper over a directory within a filesystem or a zip archive. The wrapper
//! allows introspection of the archive and reading contents. To write a zip
//! archive, you may save contents to a directory and then use the
//! [copy_archive_to_zipfile](fn.copy_archive_to_zipfile.html) function.
//! (Because any valid zip archive should be supported by `ZipDirArchive`, as an
//! alternative to using this function to create a zip archive, you may create
//! the zip file via any other means.)
//!
//! When reading zip archives, the zip file need not be a local file on the
//! filesystem. Instead,
//! [Self::from_zip](struct.ZipDirArchive.html#method.from_zip) takes any reader
//! that implements the `std::io::Read + std::io::Seek` traits. Therefore, one
//! could open files over e.g. HTTP using a reader that implements these traits.
//!
//! The development use case was to implement the
//! [`.braidz`](https://strawlab.github.io/strand-braid/braidz-files.html)
//! storage format for the [Braid](https://strawlab.org/braid) program. Braid
//! saves data during acquisition by streaming to `.csv` files (or compressed
//! `.csv.gz` files) but then, when finished, will copy these files from a plain
//! directory on disk into a `.zip` archive.
//!
//! Zip archives may be created in a custom manner to allow features such as
//! having initial data in the file which identifies the file type as something
//! beyond a plain zip file and storing files in the zip archive without
//! compression from the zip container (e.g. because the original file is
//! already compressed). Both of these features are used in the `.braidz`
//! format.
//!
//! For related ideas, see
//! [Zarr](https://zarr.readthedocs.io/en/stable/spec/v2.html) and
//! [N5](https://github.com/saalfeldlab/n5).

use std::{
    fs::File,
    io::{BufReader, Read, Seek, Write},
    path::{Component, Path, PathBuf},
};

#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

/// A type alias to wrap return types.
pub type Result<M> = std::result::Result<M, Error>;

/// The possible error types.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{source}")]
    Io {
        source: std::io::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Zip {
        source: zip::result::ZipError,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("file not found")]
    FileNotFound,
    #[error("filename not utf8")]
    FilenameNotUtf8,
    #[error("unexpected zip file content")]
    UnexpectedZipContent,
    #[error("unexpected zip file name")]
    UnexpectedZipName,
    #[error("directory does not exist: {0}")]
    NotDirectory(String),
}

impl From<zip::result::ZipError> for Error {
    fn from(source: zip::result::ZipError) -> Self {
        match source {
            zip::result::ZipError::FileNotFound => Error::FileNotFound,
            source => Error::Zip {
                source,
                #[cfg(feature = "backtrace")]
                backtrace: Backtrace::capture(),
            },
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        match source.kind() {
            std::io::ErrorKind::NotFound => Error::FileNotFound,
            _ => Error::Io {
                source,
                #[cfg(feature = "backtrace")]
                backtrace: Backtrace::capture(),
            },
        }
    }
}

#[derive(Debug)]
/// Read-access to either a single zip file or an directory.
///
/// This provides a uniform API for accessing files in a directory in a
/// conventional filesystem or accessing entries in a zip archive. See the
/// crate-level documentation for more information.
pub struct ZipDirArchive<R: Read + Seek> {
    /// The path to the archive (either zip file or dir)
    path: PathBuf,
    /// the zip archive, if this is a zip file.
    zip_archive: Option<zip::ZipArchive<R>>,
}

impl ZipDirArchive<BufReader<File>> {
    /// Automatically open a path on the filesystem as a ZipDirArchive
    ///
    /// If the path is a directory, it will be opened with
    /// [Self::from_dir](struct.ZipDirArchive.html#method.from_dir). If not, it
    /// will be opened as a file and passed to
    /// [Self::from_zip](struct.ZipDirArchive.html#method.from_zip).
    pub fn auto_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        if path.as_ref().exists() {
            if path.as_ref().is_dir() {
                Self::from_dir(path.as_ref().to_path_buf())
            } else {
                let reader = BufReader::new(File::open(&path)?);
                Self::from_zip(
                    reader,
                    path.as_ref().as_os_str().to_str().unwrap().to_string(),
                )
            }
        } else {
            Err(Error::FileNotFound)
        }
    }

    /// Open a filesystem directory as a ZipDirArchive.
    pub fn from_dir(path: PathBuf) -> Result<Self> {
        Ok(ZipDirArchive {
            path,
            zip_archive: None,
        })
    }
}

impl<R: Read + Seek> ZipDirArchive<R> {
    /// Open a reader of a zip archive as a ZipDirArchive.
    pub fn from_zip(reader: R, display_name: String) -> Result<Self> {
        let zip_archive = Some(zip::ZipArchive::new(reader)?);
        Ok(ZipDirArchive {
            path: display_name.into(),
            zip_archive,
        })
    }
    pub fn path_starter(&mut self) -> PathLike<R> {
        let parent = self;
        PathLike {
            parent,
            relname: "".into(),
        }
    }
    pub fn display(&self) -> std::path::Display<'_> {
        self.path.display()
    }
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// compute full path for non-zip file
    fn rel(&self, relname: &Path) -> PathBuf {
        match self.zip_archive {
            // zip files always use forward slash
            // https://stackoverflow.com/a/60276958
            Some(_) => self.path.slash_join(relname),
            None => self.path.join(relname),
        }
    }
    // Check if relative path exists.
    pub fn exists(&mut self, relname: &Path) -> bool {
        match &mut self.zip_archive {
            Some(zip_archive) => {
                let relname_str = relname.as_os_str().to_str().unwrap();
                zip_archive.by_name(relname_str).is_ok()
            }
            None => self.rel(relname).exists(),
        }
    }

    /// Open relative path and return reader.
    pub fn open<P: AsRef<Path>>(&mut self, relname: P) -> Result<FileReader> {
        let dirpath = self.rel(relname.as_ref());
        match &mut self.zip_archive {
            Some(zip_archive) => {
                let relname_str = relname.as_ref().as_os_str().to_str().unwrap();
                let zipfile = zip_archive.by_name(relname_str)?;
                Ok(FileReader::from_zip(zipfile)?)
            }
            None => Ok(FileReader::open_file(dirpath)?),
        }
    }

    pub fn is_file<P: AsRef<Path>>(&mut self, relname: P) -> bool {
        let dirpath = self.rel(relname.as_ref());
        match &mut self.zip_archive {
            Some(zip_archive) => {
                let relname_str = relname.as_ref().as_os_str().to_str().unwrap();
                zip_archive.by_name(relname_str).is_ok()
            }
            None => dirpath.is_file(),
        }
    }

    /// Lists, non-recursively, the paths in this directory.
    ///
    /// Note that on Windows, the result paths will have
    /// backslashes even though the zip file itself will
    /// have paths with forward slashes.
    pub fn list_paths<P: AsRef<Path> + std::fmt::Debug>(
        &self,
        relname: Option<P>,
    ) -> Result<Vec<PathBuf>> {
        // create the path we are looking for
        let dirpath = match &relname {
            Some(rn) => self.rel(rn.as_ref()),
            None => self.path.clone(),
        };
        let mut result = vec![];
        let mut unique_single_components = std::collections::BTreeSet::new();

        match &self.zip_archive {
            Some(zip_archive) => {
                // full zip fname, relative name
                let mut suffixes: Vec<PathBuf> = vec![];

                let mut found_any = false;
                for deep_path in zip_archive.file_names().map(PathBuf::from) {
                    if let Some(prefix) = &relname {
                        let (has_match, suffix) = remove_shared_prefix(&deep_path, prefix);
                        if has_match {
                            found_any = true;
                            match suffix {
                                None => {}
                                Some(trailing) => {
                                    suffixes.push(trailing);
                                }
                            }
                        }
                    } else {
                        // we have no prefix, so take either directory or path in this directory.
                        suffixes.push(deep_path);
                    }

                    // get the first component in the suffix
                    for p1 in suffixes.iter() {
                        match p1.components().next() {
                            Some(Component::Normal(next)) => {
                                unique_single_components.insert(PathBuf::from(next));
                            }
                            Some(_) | None => {
                                return Err(Error::UnexpectedZipName);
                            }
                        }
                    }
                }
                result = unique_single_components.into_iter().collect();
                if let Some(relname) = relname {
                    if result.is_empty() && !found_any {
                        return Err(not_dir_error(relname));
                    }
                }
            }
            None => {
                let dir_result = std::fs::read_dir(&dirpath);
                let dir_result = match dir_result {
                    Ok(d) => d,
                    Err(e) => {
                        match e.kind() {
                            std::io::ErrorKind::NotFound => {
                                // not found
                                return Err(not_dir_error(dirpath));
                            }
                            _ => {
                                return Err(e.into());
                            }
                        }
                    }
                };
                // list entries this directory
                for entry in dir_result {
                    let entry = entry?;
                    // remove dirpath from start of path
                    if let (has_match, Some(suffix)) = remove_shared_prefix(entry.path(), &dirpath)
                    {
                        debug_assert!(has_match);
                        result.push(suffix);
                    }
                }
            }
        }
        Ok(result)
    }
}

/// Provides a single concrete type for a normal file or a zipped file.
pub struct FileReader<'a> {
    inner: FileReaderInner<'a>,
    size: u64,
    position: u64,
}

enum FileReaderInner<'a> {
    File(BufReader<File>),
    ZipFile(Box<BufReader<zip::read::ZipFile<'a>>>),
}

impl<'a> FileReader<'a> {
    fn from_inner(inner: FileReaderInner<'a>, size: u64) -> Result<FileReader<'a>> {
        Ok(FileReader {
            inner,
            size,
            position: 0,
        })
    }
    fn open_file<P: AsRef<std::path::Path>>(path: P) -> Result<FileReader<'a>> {
        let f = File::open(path)?;
        let size = f.metadata()?.len();
        Self::from_inner(FileReaderInner::File(BufReader::new(f)), size)
    }
    fn from_zip(zipfile: zip::read::ZipFile<'a>) -> Result<FileReader<'a>> {
        let size = zipfile.size();
        Self::from_inner(
            FileReaderInner::ZipFile(Box::new(BufReader::new(zipfile))),
            size,
        )
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn position(&self) -> u64 {
        self.position
    }
}

impl<'a> Read for FileReader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n_bytes = match &mut self.inner {
            FileReaderInner::File(f) => f.read(buf)?,
            FileReaderInner::ZipFile(zf) => zf.read(buf)?,
        };
        self.position += n_bytes as u64;
        Ok(n_bytes)
    }
}

/// Check for matching prefix and, if present, return the differing suffix.
///
/// All components of the prefix must be matched for `has_match` to be true.
///
/// Note that src might have backslashes on Windows but prefix will not.
///
/// Returns `(has_match, suffix)`.
///
/// return `(true, None)` if both paths are identical.
fn remove_shared_prefix<P1: AsRef<Path>, P2: AsRef<Path>>(
    src: P1,
    prefix: P2,
) -> (bool, Option<PathBuf>) {
    // create two iterators
    let mut src_components = src.as_ref().components();
    let prefix_components = prefix.as_ref().components();

    // advance both iterators through shared prefix
    for c in prefix_components {
        let sc = src_components.next();
        match sc {
            Some(scc) => {
                if c != scc {
                    // If no match, we break.
                    return (false, None);
                }
            }
            // If no match, we break.
            None => return (false, None),
        }
    }

    let result: PathBuf = src_components.as_path().into();

    let suffix = if result.as_os_str() == std::ffi::OsStr::new("") {
        None
    } else {
        Some(result)
    };
    (true, suffix)
}

/// Compile time test that our types here implement `Send` trait.
#[test]
fn test_implements_send() {
    fn implements_send<F: Send>() {}
    implements_send::<ZipDirArchive<File>>();
    implements_send::<PathLike<File>>();
}

/// A representation of a path within the archive.
///
/// Caution: do not attempt to push directory components manually but use
/// `PathLike::push()` instead. The reason is that on Windows, backslash would
/// be used to separate directories, but in a zip file, slashes are always used.
#[derive(Debug)]
pub struct PathLike<'a, R: Read + Seek> {
    parent: &'a mut ZipDirArchive<R>,
    relname: PathBuf,
}

impl<'a, R: Read + Seek> PathLike<'a, R> {
    pub fn push<P: AsRef<std::path::Path>>(&mut self, p: P) {
        match self.parent.zip_archive {
            Some(_) => self.relname.slash_push(p),
            None => self.relname.push(p),
        }
    }
    pub fn join<P: AsRef<std::path::Path>>(mut self, p: P) -> Self {
        self.push(p);
        self
    }
    pub fn path(&mut self) -> &std::path::Path {
        &self.relname
    }
    pub fn replace(&mut self, relname: PathBuf) -> PathBuf {
        std::mem::replace(&mut self.relname, relname)
    }
    pub fn extension(&mut self) -> Option<&std::ffi::OsStr> {
        self.relname.extension()
    }
    pub fn set_extension(&mut self, e: &str) -> bool {
        self.relname.set_extension(e)
    }
    pub fn display(&self) -> std::path::Display<'_> {
        Path::display(&self.relname)
    }
    pub fn exists(&mut self) -> bool {
        self.parent.exists(&self.relname)
    }
    pub fn open(self) -> Result<FileReader<'a>> {
        self.parent.open(&self.relname)
    }
    pub fn is_file(&mut self) -> bool {
        self.parent.is_file(&self.relname)
    }
    /// Lists, non-recursively, the paths in this directory.
    pub fn list_paths(&self) -> Result<Vec<PathBuf>> {
        self.parent.list_paths(Some(&self.relname))
    }
}

trait SlashJoin {
    fn slash_join<P: AsRef<Path>>(&self, p: P) -> PathBuf;
    fn slash_push<P: AsRef<Path>>(&mut self, path: P);
}

impl SlashJoin for PathBuf {
    fn slash_join<P: AsRef<Path>>(&self, p: P) -> PathBuf {
        let mut buf = self.to_path_buf();
        buf.slash_push(p);
        buf
    }
    fn slash_push<P: AsRef<Path>>(&mut self, path: P) {
        // TODO: FIXME: xxx fix this terrible hack implementation.
        let self_str = format!("{}", self.display());
        let new_str = if self_str.is_empty() {
            std::path::PathBuf::from(path.as_ref())
        } else {
            let my_str = format!("{}/{}", self_str, path.as_ref().display());
            let remove_double = my_str.replace("//", "/");
            std::path::PathBuf::from(remove_double)
        };
        *self = new_str;
    }
}

/// Copy source `src` (dir or zip) to a new zip at `dest`.
///
/// This is a high level utility that will open an existing source, which can be
/// either a directory or a .zip file, and copy the contents into a newly
/// created zip file. The contents of the source are walked recursively to copy
/// it entirely.
pub fn copy_to_zip<P1: AsRef<Path>, P2: AsRef<Path>>(src: P1, dest: P2) -> Result<()> {
    let mut src_archive = ZipDirArchive::auto_from_path(src.as_ref()).unwrap();
    let mut zipfile = File::create(dest)?;
    copy_archive_to_zipfile(&mut src_archive, &mut zipfile)
}

/// Copy `src`, an already open archive, to `dest`, an already open file.
///
/// This utility takes an already open source archive and copies the contents
/// into an already created destination file. The contents of the source are
/// walked recursively to copy it entirely. The destination file is written to
/// but remains open with the file cursor position unmodified after finishing
/// writing the zip archive into the file. In other words, the file cursor
/// remains at the end of the open file object.
///
/// Note that it is not necessary to use this function to create a zip archive
/// which can be to opened later as a
/// [ZipDirArchive](struct.ZipDirArchive.html). This is because any zip file
/// should also be readable by
/// [ZipDirArchive::from_zip()](struct.ZipDirArchive.html#method.from_zip).
pub fn copy_archive_to_zipfile<R: Read + Seek>(
    src: &mut ZipDirArchive<R>,
    dest: &mut File,
) -> Result<()> {
    let mut zip_writer = zip::ZipWriter::new(dest);
    copy_dir(src, None, &mut zip_writer, 0)?;
    zip_writer.finish()?;
    Ok(())
}

/// copy from src into zip file
#[allow(clippy::only_used_in_recursion)]
fn copy_dir<R: Read + Seek>(
    src: &mut ZipDirArchive<R>,
    relname: Option<&Path>,
    zip_writer: &mut zip::ZipWriter<&mut File>,
    depth: usize,
) -> zip::result::ZipResult<()> {
    let parent = match relname {
        None => PathBuf::new(),
        Some(parent) => PathBuf::from(parent),
    };

    // get paths in this dir
    let paths = src
        .list_paths::<PathBuf>(relname.map(PathBuf::from))
        .unwrap();

    // iterate over entries
    for entry in paths.iter() {
        let full_entry = parent.join(entry);
        // create PathLike for entry
        let mut ep = src.path_starter();
        ep.push(full_entry.as_os_str().to_str().unwrap());

        // copy contents if it is a file
        if ep.is_file() {
            let mut fd = ep.open().unwrap();
            let mut buf = vec![];
            fd.read_to_end(&mut buf).unwrap();

            let mut options = zip::write::FileOptions::default();
            if buf.len() >= 0xFFFFFFFF {
                println!("setting large file to true");
                options = options.large_file(true);
            }

            zip_writer
                .start_file(full_entry.to_str().unwrap(), options)
                .unwrap();
            zip_writer.write_all(&buf).unwrap();
        } else {
            // if not a file, it is a subdir
            let subpath: PathBuf = match relname {
                None => full_entry,
                // Some(parent) => PathBuf::from(parent).join(entry),
                Some(_) => full_entry, //PathBuf::from(parent).join(entry),
            };

            copy_dir(src, Some(&subpath), zip_writer, depth + 1)?;
        }
    }

    Ok(())
}

fn not_dir_error<P: AsRef<Path>>(relname: P) -> Error {
    Error::NotDirectory(format!("{}", relname.as_ref().display()))
}

#[cfg(test)]
mod tests {
    use crate::*;

    fn create_files(fnames: &[&str], basepath: &Path) -> std::io::Result<()> {
        for fname in fnames {
            let f = basepath.join(fname);
            let mut fd = File::create(&f).unwrap();
            fd.write_all(fname.as_bytes())?;
        }
        Ok(())
    }

    #[test]
    fn test_remove_shared_prefix() {
        let prefix = "root";
        let a = "root/hello";
        let (has_match, actual) = remove_shared_prefix(a, prefix);
        assert!(has_match);
        assert_eq!(actual.unwrap().as_os_str().to_str().unwrap(), "hello");

        let prefix = "root/b";
        let a = "root/b/hello";
        let (has_match, actual) = remove_shared_prefix(a, prefix);
        assert!(has_match);
        assert_eq!(actual.unwrap().as_os_str().to_str().unwrap(), "hello");

        let prefix = "root/a";
        let a = "root/b/hello";
        let (has_match, actual) = remove_shared_prefix(a, prefix);
        assert!(!has_match);
        assert_eq!(actual, None);

        let prefix = "root/b";
        let a = "root/b";
        let (has_match, actual) = remove_shared_prefix(a, prefix);
        assert!(has_match);
        assert_eq!(actual, None);
    }

    // #[test]
    // fn test_back_slash_and_join_implementation() {
    //     // TODO: test that backslashes are handled OK. Specifically, Windows
    //     // should not have any trouble opening zip files made on linux.

    //     // (Since we do not make zip files in this crate, we do not need to
    //     // validate that they always have forward slashes.)
    // }

    #[test]
    fn it_works() {
        // TODO: add an empty directory, especially in a subdirectory position.

        // -----
        // create dir with files
        // -----

        /*
        The following hierarchy will be created:

        .
        ├── 1
        ├── 2
        ├── 3
        ├── subdir1
        │   ├── 4
        │   ├── 5
        │   └── 6
        └── subdir2
            ├── 7
            ├── 8
            ├── 9
            └── subsub
                ├── subsub1
                └── subsub2
        */

        // create tmp dir
        let tempdir = tempfile::tempdir().unwrap();
        let root = tempdir.into_path(); // must manually cleanup now

        // // create dir in known location
        // let root = PathBuf::from("sourcetmp");
        // std::fs::create_dir_all(&root).unwrap();

        // create files
        create_files(&["1", "2", "3"], &root).unwrap();

        // create subdir
        let subdir1 = root.join("subdir1");
        std::fs::create_dir(&subdir1).unwrap();

        // create files in subdir
        create_files(&["4", "5", "6"], &subdir1).unwrap();

        // create subdir
        let subdir2 = root.join("subdir2");
        std::fs::create_dir(&subdir2).unwrap();

        // create files in subdir
        create_files(&["7", "8", "9"], &subdir2).unwrap();

        // create second level subdir
        let subsub = subdir2.join("subsub");
        std::fs::create_dir(&subsub).unwrap();

        // create files in 2nd level subdir
        create_files(&["subsub1", "subsub2", "subsub2"], &subsub).unwrap();

        let mut dirarchive = ZipDirArchive::from_dir(root.clone()).unwrap();

        // ------
        // create zip file that is a copy of the dir
        // ------

        // Create temp zip file.
        let mut zipfile = tempfile::tempfile().unwrap();

        // // Create zip file at known location
        // let zipfilename = root.with_extension("zip");
        // let mut zipfile = File::create(&zipfilename).unwrap();

        copy_archive_to_zipfile(&mut dirarchive, &mut zipfile).unwrap();
        zipfile.seek(std::io::SeekFrom::Start(0)).unwrap();

        let mut ziparchive = ZipDirArchive::from_zip(&mut zipfile, "archive.zip".into()).unwrap();

        println!("checking zip");
        check_archive(&mut ziparchive).unwrap();

        println!("checking dirs");
        check_archive(&mut dirarchive).unwrap();

        std::fs::remove_dir_all(root).unwrap();
    }

    fn check_archive<R: Read + Seek>(archive: &mut ZipDirArchive<R>) -> Result<()> {
        let paths = archive.list_paths::<PathBuf>(None)?;
        assert_eq!(paths.len(), 5);
        assert!(paths.contains(&PathBuf::from("1")));
        assert!(paths.contains(&PathBuf::from("2")));
        assert!(paths.contains(&PathBuf::from("3")));
        assert!(paths.contains(&PathBuf::from("subdir1")));
        assert!(paths.contains(&PathBuf::from("subdir2")));

        let subs = PathBuf::from("subdir2").slash_join("subsub");

        let subpaths = archive.list_paths::<PathBuf>(Some(subs))?;
        assert_eq!(subpaths.len(), 2);
        assert!(subpaths.contains(&PathBuf::from("subsub1")));
        assert!(subpaths.contains(&PathBuf::from("subsub2")));

        for not_exist_dir in &["not-exist", "abc/def", "abc\\def"] {
            match archive
                .list_paths::<PathBuf>(Some(PathBuf::from(not_exist_dir)))
                .unwrap_err()
            {
                Error::NotDirectory(_) => {}
                _ => {
                    panic!("returned wrong error. Should return NotDirectory");
                }
            }
        }

        Ok(())
    }
}
