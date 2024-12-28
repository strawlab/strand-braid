use std::env;

// This function was modified from pkg-config-rs and should have same behavior.
#[allow(clippy::if_same_then_else, clippy::needless_bool)]
fn infer_static(name: &str) -> bool {
    if env::var_os(format!("{}_STATIC", name)).is_some() {
        true
    } else if env::var_os(format!("{}_DYNAMIC", name)).is_some() {
        false
    } else if env::var_os("PKG_CONFIG_ALL_STATIC").is_some() {
        true
    } else if env::var_os("PKG_CONFIG_ALL_DYNAMIC").is_some() {
        false
    } else {
        false
    }
}

fn main() {
    println!("cargo:rerun-if-changed=src/opencv-calibrate.cpp");

    println!("cargo:rerun-if-env-changed=OPENCV_LIB_DIR");
    println!("cargo:rerun-if-env-changed=OPENCV_VERSION");
    println!("cargo:rerun-if-env-changed=OPENCV_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=OPENCV_STATIC");
    println!("cargo:rerun-if-env-changed=OPENCV_DYNAMIC");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_ALL_STATIC");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_ALL_DYNAMIC");

    let include_paths = match env::var_os("OPENCV_LIB_DIR") {
        None => {
            // use OpenCV config from pkg-config
            let opencv = pkg_config::Config::new()
                .env_metadata(true)
                .probe("opencv4")
                .unwrap();
            println!("cargo:rustc-link-lib=z");
            opencv.include_paths
        }
        Some(opencv_libdir) => {
            // use OpenCV config from environment variable
            let libdir = std::path::Path::new(&opencv_libdir);
            let version = env::var("OPENCV_VERSION").expect("env var OPENCV_VERSION");

            let mut include_paths = vec![];
            if let Some(include_dir) = env::var_os("OPENCV_INCLUDE_DIR") {
                include_paths.push(include_dir.into());
            }

            println!("cargo:rustc-link-search=native={}", libdir.display());

            // Get static using pkg-config-rs rules.
            let statik = infer_static("OPENCV");

            // Set libname.
            if statik {
                // Tested with opencv 3.2.0 on Windows. Not working.
                // On x86_64-pc-windows-msvc, need: RUSTFLAGS="-Ctarget-feature=+crt-static"
                // this is in specified in Cargo.toml.
                println!("cargo:rustc-link-lib=static=opencv_world{}", version);
                println!("cargo:rustc-link-lib=static=zlib");
            } else {
                // tested with opencv 3.2
                println!("cargo:rustc-link-lib=opencv_world{}", version);
            }

            include_paths
        }
    };

    let mut compiler = cc::Build::new();
    compiler.file("src/opencv-calibrate.cpp");
    compiler.cpp(true);
    compiler.flag_if_supported("-std=c++11");
    for path in include_paths.iter() {
        compiler.include(path);
    }
    compiler.compile("opencv-calibrate");
}
