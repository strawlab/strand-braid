# Strand Camera and Braid .deb installer

The .deb file here installs the Strand Camera (https://strawlab.org/strand-cam/)
and Braid (https://strawlab.org/braid/) software packages.

## Prerequisites

### Pylon (Basler cameras)

To install, you must first install the package
`pylon_7.3.0.27189-deb0_amd64.deb`. Download from the file
`pylon_7.3.0.27189_linux-x86_64_debs.tar.gz` available at
https://www.baslerweb.com/en/sales-support/downloads/software-downloads/

### Vimba X (Allied Vision Technology cameras)

You must install `VimbaX_Setup-2024-1-Linux64.tar.gz`. Download from
https://www.alliedvision.com/en/products/software/vimba-x-sdk. Install like
this:

    sudo tar xzf VimbaX_Setup-2024-1-Linux64.tar.gz -C /opt

This will install the following file, among others: `/opt/VimbaX_2024-1/bin/libVmbC.so`.

## Installation

You should be able to install the .deb file by double clicking on it in a file
navigator. Alternatively, you can install it from the command line:

    sudo apt install ./strand-braid_0.12.0-alpha.8-1_amd64.deb

## Running

There are various ways to run Braid and Strand Camera. To get started, run
strand camera for your camera. For Basler cameras, the Pylon driver is used:

    strand-cam-pylon

For Allied Vision cameras, the Vimba driver is used:

    strand-cam-vimba
