use freemovr_calibration_webapp::MyWorker;
use yew_agent::PublicWorker;

fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    MyWorker::register();
}
