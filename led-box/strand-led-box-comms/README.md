# Testing

`std` is required when running the tests, but otherwise the library is `no_std`.

```
cargo test --features std
```

Test that this remains true by building for a target without std:

    cargo build --no-default-features --target thumbv7em-none-eabihf --features print-defmt
