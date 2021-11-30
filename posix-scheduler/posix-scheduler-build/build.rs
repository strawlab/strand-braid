extern crate cc;

#[cfg(not(target_family = "unix"))]
fn main() {}

#[cfg(target_family = "unix")]
fn main() {
    cc::Build::new()
        .file("src/posix_consts.c")
        .compile("posix_consts");

    #[cfg(feature = "linux")]
    cc::Build::new()
        .file("src/linux_consts.c")
        .compile("linux_consts");
}
