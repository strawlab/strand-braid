To run the example:

    cargo run --bin simple

To specify that the library directory where libcam_iface_mega is:

    export LIBRARY_PATH="/usr/local/lib"

To run the tests and see the println results:

     cargo test -- --nocapture
rustc --pretty expanded -Z unstable-options -A unstable-features src/lib.rs
rustc --pretty expanded -Z unstable-options -A unstable-features src/lib.rs


## Note about threadsafety

Note that this library wraps pointers to libcamiface with std::ptr::Unique<T>
instead of with a raw pointer. This means we are declaring the pointer to be
Send and therefore that we assume that the CamContext can be sent to different
thread with no problem. This assumption may not be true. Strange bugs when
running with multiple threads should investigate the possibility that cam_iface
is not threadsafe.
