use strand_cam_mkvfix::mkv_fix;

fn main() {
    let mut args = std::env::args_os();
    let _me = args.next().unwrap(); // get own name
    let filename = args.next().expect("need filename as argument");
    let orig_path = std::path::PathBuf::from(filename);
    assert!(
        args.next().is_none(),
        "expected only one filename as argument"
    );

    println!("fixing {}", orig_path.display());
    mkv_fix(orig_path).unwrap();
}
