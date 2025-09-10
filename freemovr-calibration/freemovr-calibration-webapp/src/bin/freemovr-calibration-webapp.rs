fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<freemovr_calibration_webapp::App>::new().render();
}
