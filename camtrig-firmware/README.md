Camera trigger firmware for Nucleo-F303RE

# Building

Make sure the rust src is installed via rustup:

    # In the directory with Cargo.toml for camtrig-firmware
    rustup component add rust-src
    rustup target add thumbv7em-none-eabihf

You may also find these Debian and Ubuntu packages useful:
`gdb-arm-none-eabi openocd qemu-system-arm binutils-arm-none-eabi`.

# debugging

    # openocd -f interface/stlink-v2-1.cfg -f target/stm32f3x.cfg
    openocd -f board/st_nucleo_f3.cfg

    arm-none-eabi-gdb target_makefile/thumbv7em-none-eabihf/debug/camtrig

# License

This code is proprietary.

Portions are drived from the cortex-m-quickstart project, which is licensed
under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)

- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
