# ads-apriltag

This Rust crate provides high level rust bindings for the
[apriltag-sys](https://crates.io/crates/apriltag-sys) crate. This allows using
the [aptriltag](https://github.com/AprilRobotics/apriltag) library from rust
without directly writing unsafe code.

Note that there is another high level rust apriltag library
[here](https://crates.io/crates/apriltag).

## Prerequisites:

This crate requires the april tag library.

On Debian/Ubuntu linux, you can install the prerequisites to build like this:

    sudo apt-get install libapriltag-dev

On Windows, you need to install pthread with vcpkg for `apriltag-sys`

    vcpkg install pthread:x64-windows-static

## Build and test

Build and test like this:

    cargo test

On Windows, tell `apriltag-sys` where to find pthread:

    $Env:APRILTAG_SYS_WINDOWS_PTHREAD_INCLUDE_DIR="/path/to/vcpkg/installed/x64-windows-static/include"
    $Env:APRILTAG_SYS_WINDOWS_PTHREAD_STATIC_LIB="/path/to/vcpkg/installed/x64-windows-static/lib/pthreadVC3.lib"

## License

Like the upstream apriltag library, this rust crate is licensed under the
BSD-2-Clause license.
