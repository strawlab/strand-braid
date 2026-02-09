use std::path::{Path, PathBuf};

use crate::NvencError;

// The dynamic loading aspects here were inspired by clang-sys.

// Due to the thread local stuff here, it is somewhat complex to abstract this
// into a standalone library.

pub struct SharedLibrary {
    pub(crate) library: libloading::Library,
    path: PathBuf,
}

impl SharedLibrary {
    fn new(library: libloading::Library, path: PathBuf) -> Self {
        Self { library, path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn load_manually() -> Result<SharedLibrary, NvencError> {
    #[cfg(target_os = "windows")]
    let path = PathBuf::from("nvEncodeAPI64.dll");
    #[cfg(not(target_os = "windows"))]
    let path = PathBuf::from("libnvidia-encode.so.1");
    let library = unsafe { libloading::Library::new(&path) }.map_err(|source| {
        NvencError::DynLibLoadError {
            dynlib: path.display().to_string(),
            source,
        }
    })?;

    let library = SharedLibrary::new(library, path);

    Ok(library)
}

pub fn load() -> Result<SharedLibrary, NvencError> {
    let library = load_manually()?;
    Ok(library)
}

#[cfg(test)]
mod tests {
    use crate::load::*;
    #[ignore = "requires nv encode shared library to be present at runtime"]
    #[test]
    fn test_load_unload() {
        load().expect("load");
    }
}
