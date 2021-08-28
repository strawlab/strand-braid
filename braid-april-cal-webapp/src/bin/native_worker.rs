use braid_april_cal_webapp::MyWorker;
use yew::agent::Threaded;

fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    MyWorker::register();
}
