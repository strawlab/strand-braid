# Strand Camera and Braid for macOS (Apple Silicon)

This zip contains portable macOS (Apple Silicon / arm64) builds of the Strand
Camera (https://strawlab.org/strand-cam/) and Braid (https://strawlab.org/braid/)
software, plus the related command-line tools. There is no installer: the
programs are self-contained binaries that run in place.

These builds are for Apple Silicon (M1 and newer). They will not run on older
Intel Macs.

## Contents

* `strand-cam`            - single-camera acquisition and tracking
* `strand-cam-flydratrax` - strand-cam with the flydratrax online tracker
* `braid` / `braid-run`   - multi-camera 3D acquisition and tracking
* various analysis and calibration tools (braidz-cli, braid-process-video,
  flytrax-csv-to-braidz, braid-april-cal-cli, strand-convert, ...)
* `libpylon-cabi-*.dylib` - the Basler/Pylon C-ABI shim (see below)
* `licenses/`             - third-party license notices

## Setup

Unzip this folder to a location of your choice (for example
`~/strand-braid`). So that `braid-run` can find and launch `strand-cam` (it
looks for it next to itself and on `PATH`), and so that you can invoke the tools
by name, add that folder to your `PATH`, or run the programs by their full path.

### Clearing the macOS quarantine (important)

These binaries are NOT code-signed or notarized. macOS Gatekeeper quarantines
files downloaded from the internet, so the first attempt to run them will be
blocked with a message like "cannot be opened because the developer cannot be
verified". Remove the quarantine attribute once, for the whole unzipped folder:

    xattr -dr com.apple.quarantine /path/to/strand-braid

After that the programs run normally.

## Running

Everything is run from the Terminal. Open Terminal in the unzipped folder and
run, for example:

    ./strand-cam --camera-backend webcam

A single `strand-cam` supports every camera backend; choose it with the
`--camera-backend` argument (`pylon`, `webcam`, or `sim`). The webcam and sim
backends work out of the box.

Braid launches one `strand-cam` subprocess per camera:

    ./braid-run run my-config.toml

## Camera driver prerequisites

### Webcams (UVC etc.) -- no prerequisites

The `webcam` backend uses the operating system's native capture interface and
needs no extra install. It is intended as a development convenience and does not
support hardware triggering, so it is not suitable for synchronized multi-camera
3D tracking.

    ./strand-cam --camera-backend webcam

### Pylon (Basler cameras)

Install the Basler Pylon runtime for macOS from
https://www.baslerweb.com/en/sales-support/downloads/software-downloads/ .

The Pylon backend loads a small public "Pylon C-ABI shim" dylib at runtime. The
matching shim (`libpylon-cabi-*.dylib`) is bundled in this folder next to
`strand-cam` and is used automatically; alternatively set the `PYLON_CABI`
environment variable to the full path of a shim dylib. See "Selecting or
upgrading the Pylon version" in the User Guide:
https://strawlab.github.io/strand-braid/installation.html

If the Pylon runtime is not installed, the webcam and sim backends still work
normally -- only `--camera-backend pylon` requires it.
