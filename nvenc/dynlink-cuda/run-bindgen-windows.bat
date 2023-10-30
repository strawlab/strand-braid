@REM Download llvm from https://releases.llvm.org/download.html
set LIBCLANG_PATH=C:\Program Files\LLVM\bin
bindgen.exe "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v11.1\include\cuda.h" --default-enum-style moduleconsts --with-derive-partialeq --distrust-clang-mangling --raw-line #![allow(dead_code,non_upper_case_globals,non_camel_case_types,non_snake_case,unused_imports)] -o src\ffi.rs
