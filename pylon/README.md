## Organization of the files

1) A collection of functions written in C++ (`src/pyloncppwrap.cpp`) that exposes
   a C API. (We do not use Basler's Pylon C API because it is not offered for
   macOS.)
2) A module `src/ffi.rs` that binds the C API (provides rust definitions
   for the functions in `pyloncppwrap`).
3) A module `src/lib.rs` that makes a safe rust wrapper
   around the (unsafe) FFI functions.

Perhaps this crate should be named something like `pylon-sys` to adhere to rust
conventions when linking a native library. However, `pyloncppwrap.cpp` actually
links several native libraries which have a C++ ABI. For example, the output of `ldd
target/debug/examples/one | grep pylon` is:

```
libpylonbase-5.0.9.so => /opt/pylon5/lib64/libpylonbase-5.0.9.so (0x00007ff399554000)
libGenApi_gcc_v3_0_Basler_pylon_v5_0.so => /opt/pylon5/lib64/libGenApi_gcc_v3_0_Basler_pylon_v5_0.so (0x00007ff398eb8000)
libGCBase_gcc_v3_0_Basler_pylon_v5_0.so => /opt/pylon5/lib64/libGCBase_gcc_v3_0_Basler_pylon_v5_0.so (0x00007ff398c9f000)
libLog_gcc_v3_0_Basler_pylon_v5_0.so => /opt/pylon5/lib64/libLog_gcc_v3_0_Basler_pylon_v5_0.so (0x00007ff397b0b000)
libMathParser_gcc_v3_0_Basler_pylon_v5_0.so => /opt/pylon5/lib64/libMathParser_gcc_v3_0_Basler_pylon_v5_0.so (0x00007ff3975f8000)
libXmlParser_gcc_v3_0_Basler_pylon_v5_0.so => /opt/pylon5/lib64/libXmlParser_gcc_v3_0_Basler_pylon_v5_0.so (0x00007ff397299000)
libNodeMapData_gcc_v3_0_Basler_pylon_v5_0.so => /opt/pylon5/lib64/libNodeMapData_gcc_v3_0_Basler_pylon_v5_0.so (0x00007ff39707f000)
```

On Debian-derived linux, get `pylon` package from [the Basler software
page](https://www.baslerweb.com/en/support/downloads/software-downloads/#type=pylonsoftware;version=all;os=linuxx86)
for example, [this deb
package](https://www.baslerweb.com/en/support/downloads/software-downloads/pylon-5-0-9-linux-x86-64-bit-debian/)
is the latest at the time of writing.

## Building

Use the PYLON_VERSION environment variable to select the version of Pylon used.

```text
# with bash
export PYLON_VERSION=6
```

```text
# in Windows Powershell
$Env:PYLON_VERSION=6
```

## Camera emulation

See [Basler's documentation](https://docs.baslerweb.com/camera-emulation.html). This can
simulate different frame rates, failures, etc.

```text
# on bash (e.g. linux)
export PYLON_CAMEMU=2
```

```text
# in Windows Powershell
$Env:PYLON_CAMEMU=2
```
