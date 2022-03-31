Camera trigger firmware for Nucleo-F303RE

# Building

Make sure the rust src is installed via rustup:

    # In the directory with Cargo.toml for led-box-firmware
    rustup component add rust-src
    rustup target add thumbv7em-none-eabihf

## Debugging with Knurling (`probe-rs`)

We use the Knurling project to facilitate debugging. `probe-rs` can be used to
debug the device from a host computer and view log messages send using the
`defmt` infrastructure.

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

### Probe: onboard STLINKv2

This is the easiest option and works with only a mini-USB cable to your device.
If `probe-run` returns with `Error: The firmware on the probe is outdated`, you
can update the STLINKv2 firmware on your Nucleo using a download from
[st.com](https://www.st.com/en/development-tools/stsw-link007.html).

# License

Portions of this project are derived from the cortex-m-quickstart project, which
is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)

- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
