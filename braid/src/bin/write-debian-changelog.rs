fn main() {
    let cli_arg1 = std::env::args().skip(1).next().unwrap();
    let date_r = std::process::Command::new("sh")
        .arg("-c")
        .arg("date -R")
        .output()
        .expect("failed to execute process");

    let date_r = String::from_utf8_lossy(&date_r.stdout);
    let s = format!(
        "{} ({}-1) xenial; urgency=low

  * New release

 -- Andrew Straw <strawman@astraw.com>  {}",
        cli_arg1,
        env!("CARGO_PKG_VERSION"),
        date_r
    );
    println!("{}", s);
}
