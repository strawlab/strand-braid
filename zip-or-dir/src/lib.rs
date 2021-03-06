#![cfg_attr(feature = "backtrace", feature(backtrace))]

use std::{
    io::{BufReader, Read, Seek},
    path::{Component, Path, PathBuf},
};

#[cfg(feature = "backtrace")]
use std::backtrace::Backtrace;

pub type Result<M> = std::result::Result<M, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{source}")]
    Io {
        #[from]
        source: std::io::Error,
        #[cfg(feature = "backtrace")]
        backtrace: Backtrace,
    },
    #[error("{source}")]
    Zip { source: zip::result::ZipError },
    #[error("file not found")]
    FileNotFound,
    #[error("filename not utf8")]
    FilenameNotUtf8,
    #[error("unexpected zip file content")]
    UnexpectedZipContent,
    #[error("unexpected zip file name")]
    UnexpectedZipName,
    #[error("attempting to list contents of non-directory")]
    NotDirectory,
}

impl From<zip::result::ZipError> for Error {
    fn from(source: zip::result::ZipError) -> Self {
        match source {
            zip::result::ZipError::FileNotFound => Error::FileNotFound,
            source => Error::Zip { source },
        }
    }
}

#[derive(Debug)]
/// Either a single zip file or an directory.
pub struct ZipDirArchive<R: Read + Seek> {
    /// The path to the archive (either zip file or dir)
    path: PathBuf,
    /// the zip archive, if this is a zip file.
    zip_archive: Option<zip::ZipArchive<R>>,
}

impl ZipDirArchive<BufReader<std::fs::File>> {
    pub fn auto_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        if path.as_ref().exists() {
            if path.as_ref().is_dir() {
                Self::from_dir(path.as_ref().to_path_buf())
            } else {
                let reader = BufReader::new(std::fs::File::open(&path)?);
                Self::from_zip(
                    reader,
                    path.as_ref().as_os_str().to_str().unwrap().to_string(),
                )
            }
        } else {
            Err(Error::FileNotFound)
        }
    }

    pub fn from_dir(path: PathBuf) -> Result<Self> {
        Ok(ZipDirArchive {
            path,
            zip_archive: None,
        })
    }
}

impl<R: Read + Seek> ZipDirArchive<R> {
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
                match zip_archive.by_name(relname_str) {
                    Ok(_) => true,
                    Err(_) => false,
                }
            }
            None => self.rel(relname).exists(),
        }
    }
    /// Open relative path and return reader.
    pub fn open<P: AsRef<Path>>(&mut self, relname: P) -> Result<BufReader<Box<dyn Read + '_>>> {
        let dirpath = self.rel(relname.as_ref());
        match &mut self.zip_archive {
            Some(zip_archive) => {
                let relname_str = relname.as_ref().as_os_str().to_str().unwrap();
                let zipfile = zip_archive.by_name(relname_str)?;
                Ok(BufReader::new(Box::new(zipfile)))
            }
            None => {
                let file = std::fs::File::open(&dirpath)?;
                Ok(BufReader::new(Box::new(file)))
            }
        }
    }

    pub fn is_file<P: AsRef<Path>>(&mut self, relname: P) -> bool {
        let dirpath = self.rel(relname.as_ref());
        match &mut self.zip_archive {
            Some(zip_archive) => {
                let relname_str = relname.as_ref().as_os_str().to_str().unwrap();
                match zip_archive.by_name(relname_str) {
                    Ok(_) => true,
                    Err(_) => false,
                }
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
        &mut self,
        relname: Option<P>,
    ) -> Result<Vec<PathBuf>> {
        // create the path we are looking for
        let dirpath = match &relname {
            Some(rn) => self.rel(rn.as_ref()),
            None => self.path.clone(),
        };
        let mut result = vec![];
        let mut unique_single_components = std::collections::BTreeSet::new();

        match &mut self.zip_archive {
            Some(zip_archive) => {
                let deep_tree_file_names: Vec<PathBuf> =
                    zip_archive.file_names().map(PathBuf::from).collect();

                // full zip fname, relative name
                let mut suffixes: Vec<PathBuf> = vec![];

                let mut found_any = false;
                for deep_path in deep_tree_file_names.into_iter() {
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
                if result.is_empty() && relname.is_some() {
                    if !found_any {
                        return Err(Error::NotDirectory);
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
                                return Err(Error::NotDirectory);
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
    implements_send::<ZipDirArchive<std::fs::File>>();
    implements_send::<PathLike<std::fs::File>>();
}

#[derive(Debug)]
pub struct PathLike<'a, R: Read + Seek> {
    parent: &'a mut ZipDirArchive<R>,
    relname: PathBuf,
}

impl<'a, R: Read + Seek> PathLike<'a, R> {
    pub fn push(&mut self, p: &str) {
        match self.parent.zip_archive {
            Some(_) => self.relname.slash_push(p),
            None => self.relname.push(p),
        }
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
    pub fn open(&mut self) -> Result<BufReader<Box<dyn Read + '_>>> {
        self.parent.open(&self.relname)
    }
    pub fn is_file(&mut self) -> bool {
        self.parent.is_file(&self.relname)
    }
    /// Lists, non-recursively, the paths in this directory.
    pub fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
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
        let new_str = if self_str == "" {
            std::path::PathBuf::from(path.as_ref())
        } else {
            let my_str = format!("{}/{}", self_str, path.as_ref().display());
            let remove_double = my_str.replace("//", "/");
            std::path::PathBuf::from(remove_double)
        };
        *self = new_str;
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use std::io::Write;

    fn create_files(fnames: &[&str], basepath: &Path) -> std::io::Result<()> {
        for fname in fnames {
            let f = basepath.join(fname);
            let mut fd = std::fs::File::create(&f).unwrap();
            // write!(&fd, "This is file {}.", fname);
            fd.write(fname.as_bytes())?;
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
        let root = tempdir.into_path();

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
        // let mut zipfile = std::fs::File::create(&zipfilename).unwrap();

        {
            let mut zip_writer = zip::ZipWriter::new(&mut zipfile);
            copy_dir(&mut dirarchive, None, &mut zip_writer, 0).unwrap();
            zip_writer.finish().unwrap();
        }
        zipfile.seek(std::io::SeekFrom::Start(0)).unwrap();

        let mut ziparchive = ZipDirArchive::from_zip(&mut zipfile, "archive.zip".into()).unwrap();

        println!("checking zip");
        check_archive(&mut ziparchive).unwrap();

        println!("checking dirs");
        check_archive(&mut dirarchive).unwrap();
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
                Error::NotDirectory => {}
                _ => {
                    panic!("returned wrong error. Should return NotDirectory");
                }
            }
        }

        Ok(())
    }

    /// copy from dirarchive into zip file
    fn copy_dir(
        diracrhive: &mut ZipDirArchive<BufReader<std::fs::File>>,
        relname: Option<&Path>,
        zip_writer: &mut zip::ZipWriter<&mut std::fs::File>,
        depth: usize,
    ) -> zip::result::ZipResult<()> {
        let parent = match relname {
            None => PathBuf::new(),
            Some(parent) => PathBuf::from(parent),
        };

        // get paths in this dir
        let paths = diracrhive
            .list_paths::<PathBuf>(relname.map(PathBuf::from))
            .unwrap();

        // iterate over entries
        for entry in paths.iter() {
            let full_entry = parent.join(entry);
            // create PathLike for entry
            let mut ep = diracrhive.path_starter();
            ep.push(full_entry.as_os_str().to_str().unwrap());

            // copy contents if it is a file
            if ep.is_file() {
                let mut fd = ep.open().unwrap();
                let mut buf = vec![];
                fd.read_to_end(&mut buf).unwrap();

                zip_writer
                    .start_file(
                        full_entry.to_str().unwrap(),
                        zip::write::FileOptions::default(),
                    )
                    .unwrap();
                zip_writer.write(&buf).unwrap();
            } else {
                // if not a file, it is a subdir
                let subpath: PathBuf = match relname {
                    None => PathBuf::from(full_entry),
                    // Some(parent) => PathBuf::from(parent).join(entry),
                    Some(_) => full_entry, //PathBuf::from(parent).join(entry),
                };

                copy_dir(diracrhive, Some(&subpath), zip_writer, depth + 1)?;
            }
        }

        Ok(())
    }
}
