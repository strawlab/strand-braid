# Installation

## Software installation

Download releases from [our releases
page](https://github.com/strawlab/strand-braid/releases).

Choose the correct installer for your operating system. We build releases of
Strand Camera and Braid for recent Ubuntu Linux Long Term Support (LTS)
releases. Each Ubuntu release has a version number (e.g. "24.04") and a code
name (e.g. "Noble Numbat"). The installers at the releases page hosted on Github
are available in the "Assets" section with names like:
`strand-braid-ubuntu-<UBUNTU_VERSION>-<STRAND_BRAID_VERSION>.zip`. Here
`UBUNTU_VERSION` could be something like `2404` which would correspond to Ubuntu
24.04. Download and expand this `.zip` file. It contains a `README.txt` file
with further instructions and a `.deb` file which can be installed by the Ubuntu
operating system by double-clicking in the file manager.

<!--

Note: the source for the README.txt files included in the installler .zip are
in _packaging/ubuntu-2404-installer-zip-readme.txt

-->

## Selecting or upgrading the Pylon version (Basler cameras)

Strand Camera and Braid talk to Basler cameras through a small *shim* library
(`libpylon-cabi`) that is loaded at runtime. The shim is what links against
Basler's proprietary Pylon SDK; the Strand Camera and Braid programs themselves
do not. This means you can change the Pylon version **without reinstalling or
rebuilding** Strand Camera or Braid — you only swap the shim and the matching
Pylon runtime.

The `.deb` package bundles a default shim built against Pylon
`7.3.0.27189`. The versioned shim is installed under `/usr/lib/strand-braid/`,
with a `libpylon-cabi.so` symlink in `/usr/lib` so the dynamic loader finds it
automatically — no environment variable or other configuration is needed. For
most users this is all you need, and you can skip the rest of this section.

### Using a different Pylon version

To run against a different Pylon version, you need two matching pieces:

1. **The Pylon runtime**, installed from Basler (for example into `/opt/pylon`).
   Install the Basler `.deb` for the version you want exactly as described under
   the Pylon prerequisite above, just with the version of your choice.
2. **A shim built against that same Pylon version.** Pre-compiled shims are
   published at
   <https://strawlab.org/assets/libpylon-cabi/precompiled/>, with names like
   `libpylon-cabi-v1-linux-x86_64-pylon_<VERSION>.so`. Download the one whose
   `<VERSION>` matches the Pylon runtime you installed. (The shim can also be
   built from source from the
   [`pylon-shimload`](https://crates.io/crates/pylon-shimload) project if a
   pre-built one is not available.)

> **The shim and the Pylon runtime must be a matched pair.** A shim built for
> Pylon `X` requires the Pylon `X` runtime to be installed and discoverable at
> runtime; otherwise loading fails. Always install the two together.

> **The `v1` in the shim filename is the shim ABI generation**, not the Pylon
> version. The Strand Camera / Braid release you have installed expects a
> specific shim ABI generation (currently `v1`). Use a shim with the matching
> ABI generation; a mismatch is reported with a clear error at startup.

Once you have downloaded the shim, point `PYLON_CABI` at it. `pylon-shimload`
checks `PYLON_CABI` before falling back to the bundled shim, so your value
always takes precedence. The most direct way is to set it on the command line
for a single run:

```ignore
PYLON_CABI=/path/to/libpylon-cabi-v1-linux-x86_64-pylon_<VERSION>.so strand-cam --camera-backend pylon
```

To make the change persistent for your shell, export it from your shell startup
file (for example `~/.bashrc`):

```ignore
export PYLON_CABI=/path/to/libpylon-cabi-v1-linux-x86_64-pylon_<VERSION>.so
```

`PYLON_CABI` is read from the process environment, so make sure it is set
wherever the software is launched — including non-interactive contexts such as a
`systemd` service, a `cron` job, or `sudo` without `-E`.

## Installing the Vimba SDK (Allied Vision cameras)

To use Allied Vision cameras (the `--camera-backend vimba` backend), install
Allied Vision's **Vimba X** SDK. Download `VimbaX_Setup-2024-1-Linux64.tar.gz`
from the [Allied Vision website](https://www.alliedvision.com/en/products/vimba-sdk/)
and follow Allied Vision's installation instructions.

> **Important:** Follow the Vimba SDK's own installation steps to the end — in
> particular, **run its GenTL path installer**. Extracting the archive alone is
> not sufficient. After extracting to `/opt`, run:
>
> ```sh
> sudo /opt/VimbaX_2024-1/cti/Install_GenTL_Path.sh
> ```
>
> This installs `/etc/profile.d/VimbaX_GenTL_Path_64bit.sh`, which sets the
> `GENICAM_GENTL64_PATH` environment variable so the SDK can find its GenTL
> *transport layers* (the `.cti` files under `/opt/VimbaX_2024-1/cti/`). Then
> **start a new login shell** (log out and back in, or open a new terminal) so
> that variable is in effect.
>
> If `GENICAM_GENTL64_PATH` does not point to the Vimba transport layers, Strand
> Camera fails to start the Vimba backend with the error `VmbErrorNoTL` ("no
> transport layer"), even though the SDK library itself loaded successfully. If
> you see that error, complete the Vimba installation as above. To set the
> variable for just one shell without the installer, export it directly:
>
> ```sh
> export GENICAM_GENTL64_PATH="/opt/VimbaX_2024-1/cti:$GENICAM_GENTL64_PATH"
> ```
>
> (The SDK also ships `Set_GenTL_Path.sh` for this, but that script must be
> sourced from within its own directory to work correctly, so the explicit
> `export` above is more reliable. The installer is the persistent fix.)

## Hardware installation

### Cameras

Currently Basler cameras are best supported. We use Basler's Pylon library to
access the cameras.

Allied Vision cameras using the Vimba X library are supported in Strand Camera
and Braid.

Consumer webcams (UVC and similar) are also supported through the `webcam`
camera backend, intended as a development convenience for running Strand Camera
without machine-vision hardware. Select it with `strand-cam --camera-backend
webcam`. See [Webcams](./hardware-selection.html#webcams) for capabilities and
limitations.

### Trigger box

Braid uses the [Straw Lab Triggerbox](https://github.com/strawlab/triggerbox)
hardware to synchronize the cameras. Two hardware variants are supported: one
based on an Arduino Nano and one based on a
[Raspberry Pi Pico](https://github.com/strawlab/triggerbox/tree/main/hardware_v3/braid-triggerbox-firmware-pico).

> **Note:** Cameras that support
> [PTP (Precision Time Protocol, IEEE 1588)](https://en.wikipedia.org/wiki/Precision_Time_Protocol)
> can synchronize themselves over the network without any additional hardware,
> making the Triggerbox unnecessary. However, PTP is not the best choice in
> every situation, so Triggerbox support remains available.

On Ubuntu, it is important to add your user to the `dialout` group so that you
can access the Triggerbox. Do so like this:

```ignore
sudo adduser <username> dialout
```

### Trigger cables

Each camera must be wired to receive the hardware trigger signal from the
Triggerbox. The Triggerbox outputs a voltage pulse on each trigger event; this
signal must be connected to the appropriate trigger input pin on every camera.

**Which pin to use depends on the camera model.** Strand Camera hard-codes the
trigger input line for each supported camera backend:

- **Allied Vision cameras (Vimba X backend):** `Line0` is used as the trigger
  source. Consult your camera's datasheet to identify which physical connector
  pin corresponds to `Line0` on your specific model.
- **Basler cameras (Pylon backend):** No trigger source line is explicitly
  overridden by Strand Camera; the camera's current `TriggerSource` setting is
  used. On most Basler cameras this defaults to `Line1`, which is typically the
  opto-isolated hardware trigger input on the multi-function I/O connector.
  Consult your camera's datasheet to confirm the correct pin.

**Building the cables.** Because camera I/O connectors vary by manufacturer and
model (Basler cameras commonly use a Hirose HR10 series connector; Allied Vision
cameras vary by model), you will generally need to build or order custom cables.
Each cable connects the Triggerbox trigger output to the appropriate trigger
input pin on a single camera, along with a common ground reference. Consult the
Triggerbox documentation and your camera's datasheet for the required voltage
and connector pinouts before building cables.
