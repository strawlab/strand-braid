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

static CURRENT_DOWNLOADS: LazyLock<
    Arc<Mutex<HashMap<(String, PathBuf), (String, Arc<Mutex<()>>)>>>,
> = LazyLock::new(|| Default::default());

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
///
/// Currently, this is done in one big chunk and thus enough memory is necessary
/// to load the entire file. A future update could read individual chunks and
/// thus reduce the memory footprint.
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
            .or_insert_with(|| {
                (
                    hash.as_string().clone(),
                    Arc::new(Mutex::new(Default::default())),
                )
            })
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
            // read it,
            let bytes = std::fs::read(dest)?;
            // and validate that it matches the checksum.
            validate(&bytes, hash)?;
        } else {
            // create the dir, if it does not already exist.
            if let Some(dest_dir) = dest.parent() {
                std::fs::create_dir_all(dest_dir)?;
            }

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

            let mut bytes = vec![];
            rdr.read_to_end(&mut bytes)?;

            // validate them,
            validate(bytes.as_ref(), hash)?;
            // and save them to disk.
            let mut fd = std::fs::File::create(dest)?;
            fd.write_all(bytes.as_ref())?;
            fd.sync_all()?;
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

fn validate(bytes: &[u8], hash: &Hash) -> Result<(), DlError> {
    match hash {
        Hash::Sha256(sum) => {
            let expected = hex::decode(sum.as_bytes())?;
            let digest = sha2::Sha256::digest(bytes);
            if &digest[..] == expected.as_slice() {
                Ok(())
            } else {
                let found = format!("{:x}", digest);
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
