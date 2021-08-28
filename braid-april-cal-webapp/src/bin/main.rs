fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::start_app::<braid_april_cal_webapp::Model>();
}
