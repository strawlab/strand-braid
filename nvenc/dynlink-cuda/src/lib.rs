//! dynamic linking of CUDA

pub mod api;
mod error;
mod ffi;
pub mod load;

pub use api::{CudaContext, CudaDevice};
pub use error::*;

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn test_version() {
        let shlib = load::load().expect("load");
        let libcuda = api::init(&shlib).expect("cuda init");
        libcuda.init(0).expect("init");

        let version = libcuda.driver_get_version().expect("driver get version");
        println!("CUDA version {}", version);
    }
}
