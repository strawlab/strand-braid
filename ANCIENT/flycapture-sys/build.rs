#[cfg(windows)]
fn failed_platform_exit(target: &str) {
    panic!("The platform {:?} is known to fail. Aborting build.", target);
}

#[cfg(windows)]
fn build() {
    let target = std::env::var("TARGET").expect("getting target");
    match target.as_ref() {
        "x86_64-pc-windows-gnu" => {failed_platform_exit(&target)},
        _ => {},
    }

    println!("cargo:rustc-link-search=native=C:\\Program Files\\Point Grey Research\\FlyCapture2\\lib64\\C");
    println!("cargo:rustc-link-lib=FlyCapture2_C");
}

#[cfg(not(windows))]
fn build() {
    println!("cargo:rustc-link-lib=flycapture-c");
}

fn main() {
    build();
}
