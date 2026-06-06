// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

fn main() {
    re_build_tools::export_build_info_vars_for_crate(env!("CARGO_PKG_NAME"));
}
