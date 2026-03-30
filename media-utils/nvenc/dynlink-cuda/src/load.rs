use std::path::{Path, PathBuf};

use crate::error::CudaError;

// The dynamic loading aspects here were inspired by clang-sys.

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

pub fn load_manually() -> Result<SharedLibrary, CudaError> {
    #[cfg(target_os = "windows")]
    let path = PathBuf::from("nvcuda.dll");
    #[cfg(not(target_os = "windows"))]
    let path = PathBuf::from("libcuda.so");
    let library = unsafe { libloading::Library::new(&path) }.map_err(|source| {
        CudaError::DynLibLoadError {
            lib: path.display().to_string(),
            source,
        }
    })?;
    let library = SharedLibrary::new(library, path);

    Ok(library)
}

pub fn load() -> Result<SharedLibrary, CudaError> {
    let library = load_manually()?;
    Ok(library)
}

#[cfg(test)]
mod tests {
    use crate::load::*;
    #[ignore = "requires CUDA shared library to be present at runtime"]
    #[test]
    fn test_load_unload() {
        load().expect("load");
    }
}
