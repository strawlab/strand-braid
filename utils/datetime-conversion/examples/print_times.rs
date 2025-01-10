// Copyright 2020-2023 Andrew D. Straw.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT
// or http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

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
