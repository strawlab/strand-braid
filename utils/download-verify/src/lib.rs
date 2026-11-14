// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
};

use sha2::Digest;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DlError {
    #[error("ureq error for url: {url}")]
    UreqError { source: ureq::Error, url: String },
    #[error("IO error")]
    IOError(#[from] std::io::Error),
    #[error("Hash mismatch (expected: {expected}, found: {found}")]
    HashMismatch { expected: String, found: String },
    #[error("Hex decode error")]
    HexDecodeError(#[from] hex::FromHexError),
    #[error("Multiple hashes for the same url and destination")]
    MultipleHashes,
}

const CHUNK_SIZE: usize = 8192;
pub enum Hash {
    Sha256(String),
}

impl Hash {
    fn as_string(&self) -> &String {
        match self {
            Hash::Sha256(sum) => sum,
        }
    }
}

type CurrentDownloads = Arc<Mutex<HashMap<(String, PathBuf), (String, Arc<Mutex<()>>)>>>;

static CURRENT_DOWNLOADS: LazyLock<CurrentDownloads> = LazyLock::new(Default::default);

/// Download a file to disk if necessary and validate it.
///
/// If multiple threads try to download the same url to the same destination,
/// they will be serialized. This prevents collision of multiple threads writing
/// to the same file at the same time. Also, this allows the second thread to
/// simply read the file after the first thread has downloaded and validated it,
/// rather than downloading it again.
///
/// The current implementation does not turn the destination into an absolute
/// path, so it is possible that two threads could specify the same destination
/// with different relative paths and thus both download the same file. However,
/// this is unlikely in practice. This problem is less trivial to solve than
/// perhaps expected because [std::fs::canonicalize] requires that the path
/// already exists, but creating files temporarily can easily lead to a race
/// condition.
pub fn download_verify<P: AsRef<Path>>(url: &str, dest: P, hash: &Hash) -> Result<(), DlError> {
    let dest = dest.as_ref().to_path_buf();
    // We use a global hashmap to track currently active downloads. The key is
    // the url and destination, and the value is the hash and a mutex. The mutex
    // is used to serialize the download and consequently to avoid a second
    // download.
    let key = (url.to_string(), dest.clone());
    let (stored_hash, file_mutex) = {
        let mut guard = CURRENT_DOWNLOADS.lock().unwrap();
        guard
            .entry(key.clone())
            .or_insert_with(|| (hash.as_string().clone(), Arc::new(Mutex::new(()))))
            .clone()
    };

    if hash.as_string() != &stored_hash {
        // The caller specified a different hash for the same url and
        // destination. This is likely a bug in the caller, so we return an
        // error rather than silently ignoring the second hash.
        return Err(DlError::MultipleHashes);
    }

    {
        // In this scope we acquire the mutex on the download.
        let _file_guard = file_mutex.lock().unwrap();

        // If the file already exists,
        if dest.exists() {
            // Validate the existing file. Read chunk-by-chunk and update hash.
            let mut hasher = sha2::Sha256::new();
            let mut rdr = std::fs::File::open(&dest)?;
            loop {
                let mut chunk = [0u8; CHUNK_SIZE];
                let n = rdr.read(&mut chunk)?;
                if n == 0 {
                    break;
                }
                let slice = &chunk[..n];
                hasher.update(slice);
            }
            validate(hasher.finalize().as_slice(), hash)?;
        } else {
            // If the file does not exist, download the contents,
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_connect(Some(std::time::Duration::from_secs(10))) // max 10 seconds
                .build()
                .into();
            let response = agent.get(url).call().map_err(|source| DlError::UreqError {
                source,
                url: url.into(),
            })?;

            let mut rdr = response.into_body().into_reader();
            // Download to a file beside the destination, which is deleted
            // again unless the checksum turns out right. Only then is it moved
            // to `dest`, so `dest` is never a partial or wrong file, whatever
            // interrupts this.
            if let Some(dest_dir) = dest.parent() {
                // create the dir, if it does not already exist.
                std::fs::create_dir_all(dest_dir)?;
            }
            let (mut fd, pending) = pending_file::PendingFile::create(dest)?;
            let mut hasher = sha2::Sha256::new();
            // Read the response chunk-by-chunk and save to disk at the same
            // time, so that we don't have to hold the entire file in memory.
            // Compute the checksum along the way.
            loop {
                let mut chunk = [0u8; CHUNK_SIZE];
                let n = rdr.read(&mut chunk)?;
                if n == 0 {
                    break;
                }
                let slice = &chunk[..n];
                hasher.update(slice);
                fd.write_all(slice)?;
            }

            // validate the response,
            validate(hasher.finalize().as_slice(), hash)?;

            // and only then put the file in place.
            fd.sync_all()?;
            drop(fd);
            pending.commit()?;
        }
    }

    {
        let mut guard = CURRENT_DOWNLOADS.lock().unwrap();
        // There is a small chance that due to a race condition another thread
        // deleted this entry already, but this is not a problem.
        guard.remove(&key)
    };

    Ok(())
}

fn validate(digest: &[u8], hash: &Hash) -> Result<(), DlError> {
    match hash {
        Hash::Sha256(sum) => {
            let expected = hex::decode(sum.as_bytes())?;
            if digest == expected.as_slice() {
                Ok(())
            } else {
                let found = hex::encode(digest);
                Err(DlError::HashMismatch {
                    expected: sum.clone(),
                    found,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    /// Serve `body` to one HTTP request on a local port and return its URL.
    fn serve_once(name: &str, body: &'static [u8]) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/{name}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
        });
        url
    }

    fn names_in(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    const HELLO_SHA256: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    /// A download is written beside its destination and moved there once its
    /// checksum is right, leaving only the destination.
    #[test]
    fn a_verified_download_is_put_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("subdir").join("hello.txt");
        crate::download_verify(
            &serve_once("good", b"hello"),
            &dest,
            &crate::Hash::Sha256(HELLO_SHA256.into()),
        )
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
        assert_eq!(names_in(dest.parent().unwrap()), ["hello.txt"]);
    }

    /// A download with the wrong checksum never appears at the destination,
    /// where the next run would trip over it, and is not left beside it.
    #[test]
    fn a_download_with_the_wrong_checksum_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("hello.txt");
        let result = crate::download_verify(
            &serve_once("bad", b"not hello"),
            &dest,
            &crate::Hash::Sha256(HELLO_SHA256.into()),
        );
        assert!(result.is_err());
        assert!(names_in(dir.path()).is_empty());
    }

    #[test]
    fn it_works() {
        crate::download_verify(
            "https://ajax.googleapis.com/ajax/libs/jquery/1.12.4/jquery.min.js",
            "scratch/jquery.min.js",
            &crate::Hash::Sha256(
                "668b046d12db350ccba6728890476b3efee53b2f42dbb84743e5e9f1ae0cc404".into(),
            ),
        )
        .unwrap();
    }
}
