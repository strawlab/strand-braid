## Prereqs

(See also https://rust-fuzz.github.io/book/afl/setup.html )

On Ubuntu:

    sudo apt install clang llvm

Install afl with:

    cargo install afl

## Build and test

Build with

     cargo afl build --features afl

Test with

    cargo afl fuzz -i in -o out ../../target/debug/flydra-mvg-fuzz-target

Re-run crashes with

    cargo afl run --bin run_crash
