use std::io::{Read, Write};

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
}

pub enum Hash {
    Sha256(String),
}

/// Download a file to disk if necessary and validate it.
///
/// Currently, this is done in one big chuck and thus enough memory
/// is necessary to load the entire file. A future update could
/// read individual chunks and thus reduce the memory footprint.
pub fn download_verify<P: AsRef<std::path::Path>>(
    url: &str,
    dest: P,
    hash: &Hash,
) -> Result<(), DlError> {
    // If the file already exists,
    if dest.as_ref().exists() {
        // read it,
        let bytes = std::fs::read(dest)?;
        // and validate that it matches the checksum.
        validate(&bytes, &hash)?;
    } else {
        // create the dir, if it does not already exist.
        if let Some(dest_dir) = dest.as_ref().parent() {
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
        validate(bytes.as_ref(), &hash)?;
        // and save them to disk.
        let mut fd = std::fs::File::create(dest)?;
        fd.write(bytes.as_ref())?;
        fd.sync_all()?;
    }
    Ok(())
}

fn validate(bytes: &[u8], hash: &Hash) -> Result<(), DlError> {
    match hash {
        &Hash::Sha256(ref sum) => {
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
