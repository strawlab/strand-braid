## Platform support

Windows, linux, and macOS are all tested.

## Building

This crate expects to find the Pylon developer kit at the usual install
location. Build with normal rust commands. For example, to run the `grab` example:

    cargo run --example grab

### On macOS

On macOS, check this:
https://github.com/basler/pypylon/issues/6#issuecomment-403090732 In other
words, do this:

```
export LD_LIBRARY_PATH=/Library/Frameworks/pylon.framework/Libraries
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
