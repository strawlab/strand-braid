fn main() {
    let date_r = std::process::Command::new("sh")
        .arg("-c")
        .arg("date -R")
        .output()
        .expect("failed to execute process");

    let date_r = String::from_utf8_lossy(&date_r.stdout);
    let s = format!(
        "flydra2 ({}-1) xenial; urgency=low

  * New release

 -- Andrew Straw <strawman@astraw.com>  {}",
        env!("CARGO_PKG_VERSION"),
        date_r
    );
    println!("{}", s);
}
