// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use eyre::Result;

pub fn braid_start(_name: &str) -> Result<()> {
    dotenv::dotenv().ok();

    if std::env::var_os("RUST_LOG").is_none() {
        // TODO: Audit that the environment access only happens in single-threaded
        // code. `braid-run` calls this from inside its tokio runtime, whose
        // worker threads already exist.
        unsafe {
            std::env::set_var(
                "RUST_LOG",
                "braid=info,flydra2=info,braid_run=info,strand_cam=info,flydra_feature_detector=info,flydra1_triggerbox=info,warn",
            )
        };
    }
    Ok(())
}
