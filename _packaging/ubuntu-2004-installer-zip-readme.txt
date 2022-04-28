# Strand Camera and Braid .deb installer

The .deb file here installs the Strand Camera (https://strawlab.org/strand-cam/)
and Braid (https://strawlab.org/braid/) software packages.

## Prerequisites

### Pylon (Basler cameras)

To install, you must first install the package
`pylon_6.2.0.21487-deb0_amd64.deb`. Download from
https://www.baslerweb.com/en/sales-support/downloads/software-downloads/

### Vimba (Allied Vision Technology cameras)

You must install  `Vimba_v5.1_Linux64.tgz`. Install like this:

    sudo mkdir -p /opt/vimba
    sudo tar xzf Vimba_v5.1_Linux64.tgz -C /opt/vimba

This will install the following file, among others: `/opt/vimba/Vimba_5_1/VimbaC/DynamicLib/x86_64bit/libVimbaC.so`.
