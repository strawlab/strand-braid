// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

#![doc = include_str!("../README.md")]

use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};

/// Appended to the destination's name to form the name of the file being
/// written.
pub const PARTIAL_SUFFIX: &str = ".partial";

/// The name under which the file destined for `dest` is written until it is
/// complete: `dest` with [`PARTIAL_SUFFIX`] appended.
pub fn partial_path<P: AsRef<Path>>(dest: P) -> PathBuf {
    let mut name = dest.as_ref().as_os_str().to_os_string();
    name.push(PARTIAL_SUFFIX);
    PathBuf::from(name)
}

/// A file being written as `<dest>.partial` beside its destination.
///
/// [`commit`](Self::commit) moves it to the destination. Dropping it without
/// doing so deletes it, unless it was made by
/// [`create_original`](Self::create_original).
#[derive(Debug)]
#[must_use = "dropping a PendingFile without calling commit() abandons the file"]
pub struct PendingFile {
    /// `None` only once the file has been committed or abandoned.
    partial: Option<tempfile::TempPath>,
    dest: PathBuf,
    replace_existing: bool,
    keep_if_unfinished: bool,
}

impl PendingFile {
    /// Start writing a file that will be moved to `dest`, replacing whatever is
    /// there, when [`commit`](Self::commit) is called.
    ///
    /// Returns the open file together with the handle that commits it. They are
    /// separate so that the file can be given away to a writer which wants to
    /// own it.
    ///
    /// A `<dest>.partial` left behind by a run which was killed is replaced. One
    /// which another process is writing right now is an error: the file is
    /// locked for as long as the returned [`File`] is open.
    pub fn create<P: AsRef<Path>>(dest: P) -> io::Result<(File, Self)> {
        Self::start(dest.as_ref(), true, false)
    }

    /// As [`create`](Self::create), but for original data, which cannot be made
    /// again.
    ///
    /// If this is dropped without [`commit`](Self::commit), `<dest>.partial` is
    /// left in place rather than deleted: whatever was written before the
    /// failure stays, under a name which says it is unfinished, to be rescued.
    ///
    /// For the same reason this refuses to start if `<dest>.partial` already
    /// exists. It may be all that is left of an earlier recording.
    pub fn create_original<P: AsRef<Path>>(dest: P) -> io::Result<(File, Self)> {
        Self::start(dest.as_ref(), true, true)
    }

    /// As [`create`](Self::create), but for a destination that must not exist.
    ///
    /// This fails right away if `dest` exists, and [`commit`](Self::commit)
    /// fails if it has come into existence in the meantime.
    pub fn create_new<P: AsRef<Path>>(dest: P) -> io::Result<(File, Self)> {
        let dest = dest.as_ref();
        // `symlink_metadata` so that a dangling symlink counts as existing.
        match std::fs::symlink_metadata(dest) {
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists", dest.display()),
            )),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Self::start(dest, false, false),
            Err(e) => Err(e),
        }
    }

    fn start(
        dest: &Path,
        replace_existing: bool,
        keep_if_unfinished: bool,
    ) -> io::Result<(File, Self)> {
        if dest.file_name().is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} does not name a file", dest.display()),
            ));
        }
        // Beside the destination, so that the two are on one filesystem and the
        // final rename cannot turn into a copy.
        let partial = partial_path(dest);

        let file = if keep_if_unfinished {
            // Never touch an unfinished file which is already there.
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&partial)
                .map_err(|e| {
                    if e.kind() == io::ErrorKind::AlreadyExists {
                        io::Error::new(
                            e.kind(),
                            format!(
                                "{} already exists. It is the unfinished data of an earlier \
                                 run: rescue or remove it first.",
                                partial.display()
                            ),
                        )
                    } else {
                        e
                    }
                })?
        } else {
            // Open without truncating: if another process is in the middle of
            // writing this very file, its data must not be destroyed before
            // the lock below has told us so.
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(&partial)?
        };
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(io::Error::new(
                    io::ErrorKind::ResourceBusy,
                    format!("{} is being written by another process", partial.display()),
                ));
            }
            // Not every filesystem can lock. Then two simultaneous writers go
            // undetected, as they did before there was a lock.
            Err(std::fs::TryLockError::Error(_)) => {}
        }
        // Ours now. Discard what a run which was killed may have left.
        file.set_len(0)?;

        Ok((
            file,
            Self {
                partial: Some(tempfile::TempPath::try_from_path(partial)?),
                dest: dest.to_path_buf(),
                replace_existing,
                keep_if_unfinished,
            },
        ))
    }

    /// Where the data is being written until [`commit`](Self::commit).
    ///
    /// For an API that insists on creating its output file itself, drop the
    /// [`File`] and pass it this path.
    pub fn path(&self) -> &Path {
        self.partial
            .as_deref()
            .expect("present until committed or dropped")
    }

    /// The destination this will be moved to.
    pub fn dest(&self) -> &Path {
        &self.dest
    }

    /// Move the file to its destination.
    ///
    /// Call this only once everything is written and the writer has finished.
    /// On failure the destination is left as it was, and the partial file is
    /// deleted or kept as for a drop.
    pub fn commit(mut self) -> io::Result<()> {
        let partial = self
            .partial
            .take()
            .expect("present until committed or dropped");
        let from = partial.to_path_buf();
        let result = if self.replace_existing {
            partial.persist(&self.dest)
        } else {
            partial.persist_noclobber(&self.dest)
        };
        result.map_err(|e| {
            // The error hands the partial path back. Putting it back lets
            // `drop` treat it as it would have without the attempt.
            self.partial = Some(e.path);
            io::Error::new(
                e.error.kind(),
                format!(
                    "moving {} into place at {}: {}",
                    from.display(),
                    self.dest.display(),
                    e.error
                ),
            )
        })
    }
}

impl Drop for PendingFile {
    fn drop(&mut self) {
        if let Some(partial) = self.partial.take()
            && self.keep_if_unfinished
        {
            // Disarm the deletion which dropping a `TempPath` performs.
            let _ = partial.keep();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    /// The names in `dir`, sorted.
    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn commit_publishes_the_file() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let dest = dir.path().join("out.csv");

        let (mut file, pending) = PendingFile::create(&dest)?;
        file.write_all(b"complete")?;
        assert!(
            !dest.exists(),
            "nothing is at the destination before commit"
        );
        assert_eq!(pending.path(), dir.path().join("out.csv.partial"));
        assert_eq!(pending.path(), partial_path(&dest));
        assert_eq!(pending.dest(), dest);

        drop(file);
        pending.commit()?;
        assert_eq!(std::fs::read(&dest)?, b"complete");
        assert_eq!(listing(dir.path()), ["out.csv"]);
        Ok(())
    }

    #[test]
    fn dropping_without_commit_leaves_nothing() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let dest = dir.path().join("out.csv");
        {
            let (mut file, _pending) = PendingFile::create(&dest)?;
            file.write_all(b"half")?;
        }
        assert!(listing(dir.path()).is_empty());
        Ok(())
    }

    /// A failed rerun must not cost the result of an earlier run.
    #[test]
    fn create_replaces_an_existing_file_only_at_commit() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let dest = dir.path().join("out.csv");
        std::fs::write(&dest, b"old")?;

        {
            let (mut file, _pending) = PendingFile::create(&dest)?;
            file.write_all(b"abandoned")?;
            assert_eq!(std::fs::read(&dest)?, b"old");
        }
        assert_eq!(std::fs::read(&dest)?, b"old");

        let (mut file, pending) = PendingFile::create(&dest)?;
        file.write_all(b"new")?;
        pending.commit()?;
        assert_eq!(std::fs::read(&dest)?, b"new");
        assert_eq!(listing(dir.path()), ["out.csv"]);
        Ok(())
    }

    #[test]
    fn create_new_refuses_an_existing_destination() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let dest = dir.path().join("out.csv");
        std::fs::write(&dest, b"old")?;

        let err = PendingFile::create_new(&dest).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(listing(dir.path()), ["out.csv"], "no partial file is left");
        Ok(())
    }

    /// The destination can appear while the file is being written, for example
    /// when two runs race. The second to commit must lose rather than overwrite.
    #[test]
    fn create_new_refuses_a_destination_that_appeared_meanwhile() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let dest = dir.path().join("out.csv");

        let (mut file, pending) = PendingFile::create_new(&dest)?;
        file.write_all(b"second")?;
        std::fs::write(&dest, b"first")?;

        let err = pending.commit().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&dest)?, b"first");
        assert_eq!(listing(dir.path()), ["out.csv"], "the partial file is gone");
        Ok(())
    }

    /// What a killed run left behind must not get in the way of the next one,
    /// nor end up in its output.
    #[test]
    fn a_stale_partial_file_is_replaced() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let dest = dir.path().join("out.csv");
        std::fs::write(partial_path(&dest), b"left by a run which was killed")?;

        let (mut file, pending) = PendingFile::create(&dest)?;
        file.write_all(b"new")?;
        pending.commit()?;
        assert_eq!(std::fs::read(&dest)?, b"new");
        assert_eq!(listing(dir.path()), ["out.csv"]);
        Ok(())
    }

    /// Two runs writing to one destination at once would share a partial file
    /// and corrupt each other. The second is refused, and the first unharmed.
    #[test]
    fn a_second_writer_to_the_same_destination_is_refused() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let dest = dir.path().join("out.csv");

        let (mut file, pending) = PendingFile::create(&dest)?;
        file.write_all(b"first")?;

        let err = PendingFile::create(&dest).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::ResourceBusy);

        pending.commit()?;
        assert_eq!(std::fs::read(&dest)?, b"first");
        assert_eq!(listing(dir.path()), ["out.csv"]);

        // Once the first is done, the destination can be written again.
        drop(file);
        let (mut file, pending) = PendingFile::create(&dest)?;
        file.write_all(b"second")?;
        pending.commit()?;
        assert_eq!(std::fs::read(&dest)?, b"second");
        Ok(())
    }

    /// Original data is kept when it could not be finished, under a name which
    /// says so, and is not overwritten by the next attempt.
    #[test]
    fn create_original_keeps_the_partial_file() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let dest = dir.path().join("movie.mp4");
        {
            let (mut file, _pending) = PendingFile::create_original(&dest)?;
            file.write_all(b"recorded before the failure")?;
        }
        assert_eq!(listing(dir.path()), ["movie.mp4.partial"]);

        let err = PendingFile::create_original(&dest).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read(partial_path(&dest))?,
            b"recorded before the failure"
        );

        // Committing moves the file into place like any other.
        let other = dir.path().join("other.mp4");
        let (mut file, pending) = PendingFile::create_original(&other)?;
        file.write_all(b"complete")?;
        pending.commit()?;
        assert_eq!(listing(dir.path()), ["movie.mp4.partial", "other.mp4"]);
        assert_eq!(std::fs::read(&other)?, b"complete");
        Ok(())
    }

    /// A path-taking API can be pointed at the partial path instead.
    #[test]
    fn the_partial_path_can_be_rewritten_by_name() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let dest = dir.path().join("out.rrd");

        let (file, pending) = PendingFile::create(&dest)?;
        drop(file);
        std::fs::write(pending.path(), b"written by path")?;
        pending.commit()?;
        assert_eq!(std::fs::read(&dest)?, b"written by path");
        Ok(())
    }

    #[test]
    fn a_destination_without_a_file_name_is_an_error() {
        let err = PendingFile::create("..").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    /// The published file must be as accessible as one made by `File::create`,
    /// not private to its owner as temporary files are.
    #[cfg(unix)]
    #[test]
    fn permissions_match_file_create() -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir()?;
        let plain = dir.path().join("plain");
        File::create(&plain)?;
        let dest = dir.path().join("pending");
        let (_file, pending) = PendingFile::create(&dest)?;
        pending.commit()?;

        let mode = |path: &Path| std::fs::metadata(path).map(|md| md.permissions().mode() & 0o777);
        assert_eq!(mode(&dest)?, mode(&plain)?);
        Ok(())
    }
}
