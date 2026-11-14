// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use eyre::Result;

/// One-time process setup shared by the `braid` executables: load a `.env`
/// file into the environment and give `RUST_LOG` a default.
///
/// # Safety
///
/// This modifies the process environment, both directly and through `dotenv`.
/// That is sound only while no other thread exists which could read or write
/// the environment, so call this first thing in `main`: before starting an
/// async runtime and before spawning any thread.
pub unsafe fn braid_start(_name: &str) -> Result<()> {
    dotenv::dotenv().ok();

    if std::env::var_os("RUST_LOG").is_none() {
        // SAFETY: the caller guarantees that this is the only thread.
        unsafe {
            std::env::set_var(
                "RUST_LOG",
                "braid=info,flydra2=info,braid_run=info,strand_cam=info,flydra_feature_detector=info,flydra1_triggerbox=info,warn",
            )
        };
    }
    Ok(())
}
