//! dynamic linking of Nvidia Encode

// regenerate ffi.rs with:
//     cd gen-nvenc-bindings
//     cargo run > ../src/ffi.rs

pub mod api;
pub mod error;
#[allow(clippy::all)]
mod ffi;
pub mod guids;
pub mod load;
mod queue;

pub use api::LibNvEncode;
pub use error::NvencError;
pub use guids::*;
pub use queue::Queue;

type NvInt = u32;

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn test_version() {
        let shlib = load::load().expect("load");
        let lib_nv_encode = api::init(&shlib).expect("nvidia-encode init");
        let _functions =
            LibNvEncode::api_create_instance(lib_nv_encode).expect("api create instance");
    }
}
