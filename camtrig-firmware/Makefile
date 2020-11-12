TARGET_DIR=target_makefile

SRCS=src/bin/camtrig-firmware.rs src/lib.rs src/led.rs \
	Cargo.toml

all: $(TARGET_DIR)/thumbv7em-none-eabihf/release/camtrig-firmware.bin
.PHONY: all

$(TARGET_DIR)/thumbv7em-none-eabihf/release/camtrig-firmware: $(SRCS)
	CARGO_TARGET_DIR=$(TARGET_DIR) cargo build --release
	# install with:
	#     cargo install cargo-binutils
	#     rustup component add llvm-tools-preview
	CARGO_TARGET_DIR=$(TARGET_DIR) cargo size --bin camtrig-firmware --release

$(TARGET_DIR)/thumbv7em-none-eabihf/release/camtrig-firmware.bin: $(TARGET_DIR)/thumbv7em-none-eabihf/release/camtrig-firmware
	# install with:
	#     cargo install cargo-binutils
	#     rustup component add llvm-tools-preview
	CARGO_TARGET_DIR=$(TARGET_DIR) cargo objcopy --bin camtrig-firmware --release -- -O binary $@
