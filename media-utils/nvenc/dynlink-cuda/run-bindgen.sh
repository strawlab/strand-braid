#!/bin/bash
set -euo pipefail

# ensure bindgen 0.72.x is installed.
bindgen --version | grep -Eq '^bindgen 0\.72\.[0-9]+$'

bindgen /usr/local/cuda-10.0/include/cuda.h \
    --default-enum-style moduleconsts \
    --distrust-clang-mangling \
    --allowlist-function "cu.*" \
    --allowlist-type "CU.*" \
    --allowlist-type "cu.*" \
    --allowlist-type "cuda.*" \
    --allowlist-type "CUDA.*" \
    --allowlist-var "CU.*" \
    --allowlist-var "cuda.*" \
    --raw-line '#![allow(dead_code,non_upper_case_globals,non_camel_case_types,non_snake_case,unused_imports)]' \
    -o src/ffi.rs
