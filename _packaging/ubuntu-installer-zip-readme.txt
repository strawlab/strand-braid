# Strand Camera and Braid .deb installer

The .deb file here installs the Strand Camera (https://strawlab.org/strand-cam/)
and Braid (https://strawlab.org/braid/) software packages.

## Prerequisites

### Pylon (Basler cameras)

To use Basler cameras making use of Basler's Pylon drivers, To install, you must
first install the package `pylon_7.3.0.27189-deb0_amd64.deb`. Download from the
file `pylon_7.3.0.27189_linux-x86_64_debs.tar.gz` available at
https://www.baslerweb.com/en/sales-support/downloads/software-downloads/

This .deb bundles a Pylon shim built for Pylon 7.3.0.27189 (installed under
/usr/lib/strand-braid/) and selects it automatically. To run against a different
Pylon version, install the matching Pylon runtime and a matching shim and set
the PYLON_CABI environment variable. See "Selecting or upgrading the Pylon
version" in the User Guide:
https://strawlab.github.io/strand-braid/installation.html

### Vimba (Allied Vision Technology cameras)

To use Allied Vision Technology (AVT) cameras making use of AVT's Vimba drivers,
you must install `VimbaX_Setup-2024-1-Linux64.tar.gz`. Download from
https://www.alliedvision.com/en/products/vimba-sdk/. Install like this:

    sudo tar xzf VimbaX_Setup-2024-1-Linux64.tar.gz -C /opt

This will install the following file, among others: `/opt/VimbaX_2024-1/bin/libVmbC.so`.

Then complete the Vimba installation by running its GenTL path installer, which
sets up the GENICAM_GENTL64_PATH environment variable so the SDK can find its
GenTL transport layers (the `.cti` files under `/opt/VimbaX_2024-1/cti/`):

    sudo /opt/VimbaX_2024-1/cti/Install_GenTL_Path.sh

Log out and back in (or open a new terminal) afterwards so the variable takes
effect. If you skip this step, the Vimba backend fails to start with the error
"VmbErrorNoTL" even though the library itself loaded successfully.

## Installation

You should be able to install the .deb file by double clicking on it in a file
navigator. Alternatively, you can install it from the command line:

    sudo apt install ./@STRAND_BRAID_DEB_FILENAME@

## Running

There are various ways to run Braid and Strand Camera. To get started, run
strand camera for your camera. A single `strand-cam` program supports both
camera vendors; choose the driver with the `--camera-backend` argument.

For Basler cameras, the Pylon driver is used (this is also the default when
`--camera-backend` is omitted):

    strand-cam --camera-backend pylon

For Allied Vision cameras, the Vimba driver is used:

    strand-cam --camera-backend vimba
