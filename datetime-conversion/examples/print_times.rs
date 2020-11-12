extern crate chrono;
extern crate datetime_conversion;

use datetime_conversion::{datetime_to_f64, f64_to_datetime};

fn main() {
    let now = chrono::Local::now();
    println!("now {:?}", now);
    let now_f64 = datetime_to_f64(&now);
    println!("now_f64 {:?}", now_f64);
    let dt = f64_to_datetime(now_f64);
    println!("dt {:?}", dt);
}
