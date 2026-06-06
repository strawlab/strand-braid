// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<freemovr_calibration_webapp::App>::new().render();
}
