@REM Download https://github.com/ShiftMediaProject/libvpx/releases/download/v1.10.0/libvpx_v1.10.0_msvc16.zip
@REM and unzip into %HomeDrive%%HomePath%\libvpx_v1.10.0_msvc16
set VPX_VERSION=1.10.0
set VPX_STATIC=1
set VPX_LIB_DIR=%HomeDrive%%HomePath%\libvpx_v1.10.0_msvc16\lib\x64
set VPX_INCLUDE_DIR=%HomeDrive%%HomePath%\libvpx_v1.10.0_msvc16\include
SET VPX_NO_PKG_CONFIG=1
cargo run --no-default-features --release -- --config .\tests\montage.toml
