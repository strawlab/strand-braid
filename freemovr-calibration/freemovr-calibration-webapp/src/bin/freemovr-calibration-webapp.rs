fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<freemovr_calibration_webapp::Model>::new().render();
}
