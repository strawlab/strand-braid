# Strand Camera and Braid .deb installer

The .deb file here installs the Strand Camera (https://strawlab.org/strand-cam/)
and Braid (https://strawlab.org/braid/) software packages.

## Prerequisites

### Pylon (Basler cameras)

To install, you must first install the package
`pylon_6.2.0.21487-deb0_amd64.deb`. Download from
https://www.baslerweb.com/en/sales-support/downloads/software-downloads/

### Vimba (Allied Vision Technology cameras)

You must install  `Vimba64_v6.0_Linux.tgz`. Install like this:

    sudo mkdir -p /opt/vimba
    sudo tar xzf Vimba64_v6.0_Linux.tgz -C /opt/vimba

This will install the following file, among others: `/opt/vimba/Vimba_6_0/VimbaC/DynamicLib/x86_64bit/libVimbaC.so`.

## Installation

You should be able to install the .deb file by double clicking on it in a file
navigator. Alternatively, you can install it from the command line:

    sudo apt install ./strand-braid_0.12.0-alpha.3-1_amd64.deb

## Running

There are various ways to run Braid and Strand Camera. To get started, run
strand camera for your camera. For Basler cameras, the Pylon driver is used:

    strand-cam-pylon

For Allied Vision cameras, the Vimba driver is used:

    strand-cam-vimba
