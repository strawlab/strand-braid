#!/bin/bash
set -o errexit

bindgen /usr/local/cuda-10.0/include/cuda.h \
    --default-enum-style moduleconsts \
    --with-derive-partialeq \
    --distrust-clang-mangling \
    --raw-line '#![allow(dead_code,non_upper_case_globals,non_camel_case_types,non_snake_case,unused_imports)]' \
    -o src/ffi.rs
