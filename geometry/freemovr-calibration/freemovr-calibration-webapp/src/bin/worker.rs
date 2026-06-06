// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

use freemovr_calibration_webapp::agent::MyWorker;
use yew_agent::Registrable;

fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    MyWorker::registrar()
        .encoding::<yew_agent::Bincode>()
        .register();
}
