fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::Renderer::<braid_april_cal_webapp::Model>::new().render();
}
