fn main() {
    let mut args = std::env::args().skip(1);
    let cli_arg1 = args.next().unwrap();
    let cli_arg2 = args.next().unwrap();

    let now: chrono::DateTime<chrono::Utc> = chrono::Utc::now();
    let local = now.with_timezone(&chrono::Local);

    // Equivalent of `date -R` on linux, e.g. "Sun, 24 Dec 2017 05:19:22 +0100".
    let date_r = format!("{}", local.format("%a, %d %b %Y %H:%M:%S %z"));
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
