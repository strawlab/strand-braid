TARGET_DIR=target_makefile

SRCS=src/main.rs Cargo.toml

all: $(TARGET_DIR)/thumbv7em-none-eabihf/release/led-box-firmware.bin
.PHONY: all

$(TARGET_DIR)/thumbv7em-none-eabihf/release/led-box-firmware: $(SRCS)
	CARGO_TARGET_DIR=$(TARGET_DIR) cargo build --release
	# install with:
	#     cargo install cargo-binutils
	#     rustup component add llvm-tools-preview
	CARGO_TARGET_DIR=$(TARGET_DIR) cargo size --bin led-box-firmware --release

$(TARGET_DIR)/thumbv7em-none-eabihf/release/led-box-firmware.bin: $(TARGET_DIR)/thumbv7em-none-eabihf/release/led-box-firmware
	# install with:
	#     cargo install cargo-binutils
	#     rustup component add llvm-tools-preview
	CARGO_TARGET_DIR=$(TARGET_DIR) cargo objcopy --bin led-box-firmware --release -- -O binary $@
