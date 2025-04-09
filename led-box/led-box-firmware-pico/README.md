Optogenetics LED control firmware for Raspberry Pi Pico

# Building

To build the ELF executable, install the following:

    # In the directory with Cargo.toml for led-box-firmware-pico
    rustup component add rust-src
    rustup target add thumbv6m-none-eabi

To convert the ELF executable to a .bin file which can be copied onto the
nucleo, install the following:

    cargo install cargo-binutils
    rustup component add llvm-tools-preview

To build the ELF executable and convert it to a .bin file, do this:

    cargo objcopy --bin led-box-firmware-pico --release -- -O binary ./target/thumbv6m-none-eabi/release/led-box-firmware-pico.bin

The file at `./target/thumbv6m-none-eabi/release/led-box-firmware-pico.bin` can
now be copied onto the emulated USB mass storage device of the Nucleo board.

## Debugging with Knurling (`probe-rs`)

We use the Knurling project to facilitate debugging. `probe-rs` can be used to
debug the device from a host computer and view log messages send using the
`defmt` infrastructure. Install `probe-run` with `cargo install probe-run`.

To see `defmt` messages, compile with the `DEFMT_LOG` environment variable
set appropriately. (By default, `defmt` will show only error level messages.)

Powershell (Windows)
```
$Env:DEFMT_LOG="trace"
```

Bash (Linux/macOS)
```
export DEFMT_LOG=trace
```

### Probe

Run with:

```
cargo run --release
```

# License

Portions of this project are derived from the cortex-m-quickstart project, which
is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)

- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
