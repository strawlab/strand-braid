use fastimage::{ripp, IppVersion};

fn main() {
    ripp::init().unwrap();
    let version = IppVersion::new();
    println!("IPP version: {:?}", version);
}
