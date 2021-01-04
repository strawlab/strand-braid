fn main() {
    let mut args = std::env::args().skip(1);
    let cli_arg1 = args.next().unwrap();
    let cli_arg2 = args.next().unwrap();
    let date_r = std::process::Command::new("sh")
        .arg("-c")
        .arg("date -R")
        .output()
        .expect("failed to execute process");

    let date_r = String::from_utf8_lossy(&date_r.stdout);
    let s = format!(
        "{} ({}-1) {}; urgency=low

  * New release

 -- Andrew Straw <strawman@astraw.com>  {}",
        cli_arg1,
        env!("CARGO_PKG_VERSION"),
        cli_arg2,
        date_r
    );
    println!("{}", s);
}
