fn main() {
    println!("cargo:rustc-link-lib=cam_iface_mega");
}

/*
This is what I used on my Mac:
fn main() {
    println!("cargo:rustc-link-search=native={}","/Users/straw/Documents/src/motmot/libcamiface/src/Release");
    println!("cargo:rustc-link-lib=cam_iface_dc1394");
}
*/