// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CudaError {
    #[error("dynamic library `{lib}` could not be loaded: `{source}`")]
    DynLibLoadError {
        lib: String,
        source: libloading::Error,
    },
    #[error("CUDA returned code `{status}`")]
    ErrCode { status: u32 },
    #[error("Name `{name}` could not be opened: `{source}`")]
    NameFFIError {
        name: String,
        source: libloading::Error,
    },
}
