use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    // Put the linker script somewhere the linker can find it
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());

    #[cfg(feature = "nucleo64")]
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(include_bytes!("stm32f303re.x"))
        .unwrap();

    #[cfg(feature = "nucleo32")]
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(include_bytes!("stm32f303k8.x"))
        .unwrap();

    println!("cargo:rustc-link-search={}", out.display());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=memory.x");
}
