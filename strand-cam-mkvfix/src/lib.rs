use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("io error")]
    Io(#[from] std::io::Error),
    #[error("file does not exist")]
    FileDoesNotExist,
}

/// Split `path` (which must be a file) into directory and filename component.
fn split_path<P: AsRef<std::path::Path>>(path: P) -> (std::path::PathBuf, std::path::PathBuf) {
    let path = path.as_ref();
    assert!(path.is_file());
    let mut components = path.components();
    let filename = components.next_back().unwrap().as_os_str().into();
    let dirname = components.as_path().into();
    (dirname, filename)
}

/// Fix .mkv file so that seek works in VLC.
pub fn mkv_fix<P: AsRef<std::path::Path> + AsRef<std::ffi::OsStr>>(
    orig_path: P,
) -> Result<(), Error> {
    let opp: &std::path::Path = orig_path.as_ref();

    if !opp.is_file() {
        return Err(Error::FileDoesNotExist);
    }

    let (dir_path, filename) = split_path(opp);

    let mut new_filename = std::ffi::OsString::from("._fixed_");
    new_filename.push(filename);
    let new_path = dir_path.join(new_filename);

    // Run ffmpeg over the file.
    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.arg("-i");
    cmd.arg(&orig_path);
    cmd.arg("-vcodec");
    cmd.arg("copy");
    cmd.arg("-map_metadata");
    cmd.arg("0:g");
    cmd.arg(&new_path);
    cmd.output()?;

    // Overwrite the original file.
    std::fs::rename(new_path, &orig_path)?;
    Ok(())
}

/// Check if ffmpeg program is available.
pub fn is_ffmpeg_available() -> bool {
    match std::process::Command::new("ffmpeg").arg("-h").output() {
        Ok(_) => true,
        Err(_) => false,
    }
}
