extern crate fastimage;
extern crate ipp_sys as ipp;

use fastimage::{ripp, IppVersion};

fn main() {
    ripp::init().unwrap();
    let version = IppVersion::new();
    println!("IPP version: {:?}", version);
}
