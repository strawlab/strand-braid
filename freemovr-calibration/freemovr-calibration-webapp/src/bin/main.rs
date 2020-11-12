fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::start_app::<freemovr_calibration_webapp::Model>();
}
