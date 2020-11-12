@REM Download llvm from https://releases.llvm.org/download.html
set LIBCLANG_PATH=C:\Program Files\LLVM\bin

cd gen-nvenc-bindings
cargo run -- ..\src\ffi.rs
