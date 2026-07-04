# Strand Camera and Braid for Windows (x86_64)

This zip contains portable Windows (64-bit) builds of the Strand Camera
(https://strawlab.org/strand-cam/) and Braid (https://strawlab.org/braid/)
software, plus the related command-line tools. There is no installer: the
programs are self-contained `.exe` files that run in place.

## Contents

* `strand-cam.exe`            - single-camera acquisition and tracking
* `strand-cam-flydratrax.exe` - strand-cam with the flydratrax online tracker
* `braid.exe` / `braid-run.exe` - multi-camera 3D acquisition and tracking
* various analysis and calibration tools (braidz-cli, braid-process-video,
  flytrax-csv-to-braidz, braid-april-cal-cli, strand-convert, ...)
* `licenses\`                 - third-party license notices

## Setup

Unzip this folder to a location of your choice (for example
`C:\strand-braid`). So that `braid-run.exe` can find and launch `strand-cam.exe`
(it looks for it next to itself and on `PATH`), and so that you can invoke the
tools by name, add that folder to your `PATH`, or run the programs by their full
path.

## Running

Everything is run from the Windows command line -- open PowerShell or the
Command Prompt (`cmd.exe`) in the unzipped folder and run, for example:

    strand-cam.exe --camera-backend webcam

A single `strand-cam.exe` supports every camera backend; choose it with the
`--camera-backend` argument (`pylon`, `vimba`, `webcam`, or `sim`). The
webcam, vimba, and sim backends work out of the box. The Basler/Pylon backend
additionally requires the Pylon runtime and the Pylon C-ABI shim (see below).

Braid is run the same way as on Linux; it launches one `strand-cam.exe`
subprocess per camera:

    braid-run.exe run my-config.toml

## Camera driver prerequisites

### Webcams (UVC etc.) -- no prerequisites

The `webcam` backend uses the operating system's native capture interface
(Media Foundation on Windows) via the bundled code and needs no extra install.
It is intended as a development convenience and does not support hardware
triggering, so it is not suitable for synchronized multi-camera 3D tracking.

    strand-cam.exe --camera-backend webcam

### Vimba (Allied Vision cameras)

Install the Allied Vision Vimba X SDK for Windows from
https://www.alliedvision.com/en/products/vimba-sdk/ . The Vimba backend loads
`VmbC.dll` at runtime from the default install location
(`C:\Program Files\Allied Vision\Vimba X\bin\VmbC.dll`).

    strand-cam.exe --camera-backend vimba

### Pylon (Basler cameras)

Install the Basler Pylon runtime for Windows from
https://www.baslerweb.com/en/sales-support/downloads/software-downloads/ .

In addition, the Pylon backend loads a small public "Pylon C-ABI shim" DLL at
runtime. If a `libpylon-cabi-*.dll` file is present in this folder (next to
`strand-cam.exe`), it is used automatically; otherwise set the `PYLON_CABI`
environment variable to the full path of a matching shim DLL. See "Selecting or
upgrading the Pylon version" in the User Guide:
https://strawlab.github.io/strand-braid/installation.html

If no Pylon shim is available, the other backends (webcam, vimba, sim) still
work normally -- only `--camera-backend pylon` requires it.
