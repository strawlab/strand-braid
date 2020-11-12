extern crate rustc_serialize;
use rustc_serialize::json;

extern crate simple_vr;

use simple_vr::config;

fn main() {
    let cfg = config::Config::default();
    let encoded = json::as_pretty_json(&cfg);
    println!("{}", encoded);
}
