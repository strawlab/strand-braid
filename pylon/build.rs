extern crate cc;

#[cfg(not(any(target_os="linux", target_os="macos", target_os="windows")))]
compile_error!("Currently only linux, macos, and windows support implemented.");

#[cfg(target_os = "linux")]
enum PylonVersion {
    V5_0,
    V5_1,
    V5_2,
    Unknown,
}

fn main() {
    println!("cargo:rerun-if-env-changed=PYLON_VERSION");

    #[allow(unused_variables)]
    let pylon_version: u8 = std::env::var_os("PYLON_VERSION")
    .map(|s| {
        s.into_string()
            .expect("If set, PYLON_VERSION environment variable must be an integer between 0-255.")
        .parse()
            .expect("If set, PYLON_VERSION environment variable must be an integer between 0-255.")
    })
    .unwrap_or(5);

    #[cfg(target_os = "linux")]
    {
        let pylon_root = match std::env::var("PYLON_ROOT") {
            Ok(val) => val,
            Err(_) => "/opt/pylon5".into(),
        };
        let pylon_root = std::path::PathBuf::from(pylon_root);
        let mut lib_dir = pylon_root.clone();
        lib_dir.push("lib64");
        match lib_dir.to_str() {
            Some(dir_str) => {

                let mut so_file_for_5_2 = lib_dir.clone();
                so_file_for_5_2.push("libpylon_TL_usb-5.2.0.so");

                let mut so_file_for_5_1 = lib_dir.clone();
                so_file_for_5_1.push("libGenApi_gcc_v3_1_Basler_pylon_v5_1");
                so_file_for_5_1.set_extension("so");

                eprint!("# pylon build: checking for file {}...", so_file_for_5_2.display());
                let version = if so_file_for_5_2.exists() {
                    eprintln!("found");
                    PylonVersion::V5_2
                } else {
                    eprintln!("not found");

                    eprint!("# pylon build: checking for file {}...", so_file_for_5_1.display());
                    if so_file_for_5_1.exists() {
                        eprintln!("found");
                        PylonVersion::V5_1
                    } else {
                        eprintln!("not found");
                        let mut so_file_for_5_0 = lib_dir.clone();
                        so_file_for_5_0.push("libGenApi_gcc_v3_0_Basler_pylon_v5_0");
                        so_file_for_5_0.set_extension("so");
                        eprint!("# pylon build: checking for file {}...", so_file_for_5_0.display());
                        if so_file_for_5_0.exists() {
                            eprintln!("found");
                            PylonVersion::V5_0
                        } else  {
                            eprintln!("not found");
                            PylonVersion::Unknown
                        }
                    }
                };

                println!("cargo:rustc-link-search=native={}", dir_str);
                println!("cargo:rustc-link-lib=pylonc");

                // The Basler docs want the rest of these libraries to be automatically
                // found using rpath linker args, but sending options to the linker in rust
                // requires the unstable link_args feature. So we specify them manually.
                // See https://github.com/rust-lang/cargo/issues/5077
                println!("cargo:rustc-link-lib=pylonbase");
                println!("cargo:rustc-link-lib=pylonutility");
                println!("cargo:rustc-link-lib=gxapi");

                match version {
                    PylonVersion::V5_0 => {
                        println!("cargo:rustc-link-lib=GenApi_gcc_v3_0_Basler_pylon_v5_0");
                        println!("cargo:rustc-link-lib=GCBase_gcc_v3_0_Basler_pylon_v5_0");
                        println!("cargo:rustc-link-lib=Log_gcc_v3_0_Basler_pylon_v5_0");
                        println!("cargo:rustc-link-lib=MathParser_gcc_v3_0_Basler_pylon_v5_0");
                        println!("cargo:rustc-link-lib=XmlParser_gcc_v3_0_Basler_pylon_v5_0");
                        println!("cargo:rustc-link-lib=NodeMapData_gcc_v3_0_Basler_pylon_v5_0");
                    }
                    PylonVersion::V5_1 => {
                        println!("cargo:rustc-link-lib=GenApi_gcc_v3_1_Basler_pylon_v5_1");
                        println!("cargo:rustc-link-lib=GCBase_gcc_v3_1_Basler_pylon_v5_1");
                        println!("cargo:rustc-link-lib=Log_gcc_v3_1_Basler_pylon_v5_1");
                        println!("cargo:rustc-link-lib=MathParser_gcc_v3_1_Basler_pylon_v5_1");
                        println!("cargo:rustc-link-lib=XmlParser_gcc_v3_1_Basler_pylon_v5_1");
                        println!("cargo:rustc-link-lib=NodeMapData_gcc_v3_1_Basler_pylon_v5_1");
                    }
                    PylonVersion::V5_2 => {
                        println!("cargo:rustc-link-lib=GenApi_gcc_v3_1_Basler_pylon");
                        println!("cargo:rustc-link-lib=GCBase_gcc_v3_1_Basler_pylon");
                        println!("cargo:rustc-link-lib=Log_gcc_v3_1_Basler_pylon");
                        println!("cargo:rustc-link-lib=MathParser_gcc_v3_1_Basler_pylon");
                        println!("cargo:rustc-link-lib=XmlParser_gcc_v3_1_Basler_pylon");
                        println!("cargo:rustc-link-lib=NodeMapData_gcc_v3_1_Basler_pylon");
                    }
                    PylonVersion::Unknown => {
                        panic!("could not detect pylon library version");
                    }
                }
            }
            None => {
                panic!("could not compute pylon lib dir (is it UTF-8?)");
            }
        }

        cc::Build::new()
            .file("src/pyloncppwrap.cpp")
            .warnings(false)
            .cpp(true)
            .include("/opt/pylon5/include")
            .include("/opt/pylon5/include/pylon")
            .compile("pyloncppwrap");
    }

    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-search=framework=/Library/Frameworks/");
        println!("cargo:rustc-link-lib=framework=pylon");
        cc::Build::new()
            .file("src/pyloncppwrap.cpp")
            .warnings(false)
            .cpp(true)
            .include("/Library/Frameworks/pylon.framework/Headers/GenICam")
            .include("/Library/Frameworks/pylon.framework/Headers")
            .flag("-F/Library/Frameworks")
            .compile("pyloncppwrap");
    }

    #[cfg(target_os = "windows")]
    {
        use std::path::PathBuf;

        let pylon_dev_dir = match pylon_version {
            5 => PathBuf::from(r#"C:\Program Files\Basler\pylon 5\Development"#),
            6 => PathBuf::from(r#"C:\Program Files\Basler\pylon 6\Development"#),
            version => panic!("unsupported pylon version: {}", version),
        };

        let mut include_dir = pylon_dev_dir.clone();
        include_dir.push("include");

        let mut pylon_include_dir = include_dir.clone();
        pylon_include_dir.push("pylon");

        let mut lib_dir = pylon_dev_dir.clone();
        lib_dir.push("lib");
        lib_dir.push("x64");

        println!("cargo:rustc-link-search={}", lib_dir.display());
        cc::Build::new()
            .file("src/pyloncppwrap.cpp")
            // .warnings(false)
            .cpp(true)
            .include(&include_dir)
            .include(&pylon_include_dir)
            .compile("pyloncppwrap");
    }

}
