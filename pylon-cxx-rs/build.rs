fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=include/catcher.h");
    println!("cargo:rerun-if-changed=include/pylon-cxx-rs.h");
    println!("cargo:rerun-if-changed=src/pylon-cxx-rs.cc");

    let mut build = cxx_build::bridge("src/lib.rs");

    build
        .file("src/pylon-cxx-rs.cc")
        .warnings(false)
        .cpp(true)
        .include("include".to_string());

    #[cfg(target_os = "linux")]
    {
        let pylon_root = std::path::PathBuf::from("/opt/pylon");

        let include1 = pylon_root.join("include");

        build.flag("-std=c++14").include(&include1);

        let mut lib_dir = pylon_root.clone();
        lib_dir.push("lib");

        let dir_str = lib_dir.to_str().unwrap();

        println!("cargo:rustc-link-search=native={}", dir_str);
        println!("cargo:rustc-link-lib=pylonc");

        // The Basler docs want the rest of these libraries to be automatically
        // found using rpath linker args, but sending options to the linker in rust
        // requires the unstable link_args feature. So we specify them manually.
        // See https://github.com/rust-lang/cargo/issues/5077
        println!("cargo:rustc-link-lib=pylonbase");
        println!("cargo:rustc-link-lib=pylonutility");
        println!("cargo:rustc-link-lib=gxapi");

        // The following are for Pylon 6.1 and may need to be updated for other versions.
        println!("cargo:rustc-link-lib=GenApi_gcc_v3_1_Basler_pylon");
        println!("cargo:rustc-link-lib=GCBase_gcc_v3_1_Basler_pylon");
        println!("cargo:rustc-link-lib=Log_gcc_v3_1_Basler_pylon");
        println!("cargo:rustc-link-lib=MathParser_gcc_v3_1_Basler_pylon");
        println!("cargo:rustc-link-lib=XmlParser_gcc_v3_1_Basler_pylon");
        println!("cargo:rustc-link-lib=NodeMapData_gcc_v3_1_Basler_pylon");
    }

    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-search=framework=/Library/Frameworks/");
        println!("cargo:rustc-link-lib=framework=pylon");

        build
            .flag("-std=c++14")
            .include("/Library/Frameworks/pylon.framework/Headers/GenICam")
            .include("/Library/Frameworks/pylon.framework/Headers")
            .flag("-F/Library/Frameworks");
    };

    #[cfg(target_os = "windows")]
    {
        use std::path::PathBuf;

        let pylon_dev_dir = PathBuf::from(r#"C:\Program Files\Basler\pylon 6\Development"#);

        let mut include_dir = pylon_dev_dir.clone();
        include_dir.push("include");

        let mut pylon_include_dir = include_dir.clone();
        pylon_include_dir.push("pylon");

        let mut lib_dir = pylon_dev_dir.clone();
        lib_dir.push("lib");
        lib_dir.push("x64");

        println!("cargo:rustc-link-search={}", lib_dir.display());

        build.include(include_dir);
    }

    build.compile("pyloncxxrs-demo");
}
