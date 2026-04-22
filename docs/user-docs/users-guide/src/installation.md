# Installation

## Software installation

Download releases from [our releases
page](https://github.com/strawlab/strand-braid/releases).

Choosing the correct installer for your operating system. We build releases of
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

## Hardware installation

### Cameras

Currently Basler cameras are best supported. We use Basler's Pylon library to
access the cameras.

Allied Vision Cameras using the Vimba X library is planned for Strand Camera and
Braid version 0.12.

### Trigger box

Braid uses the [Straw Lab Triggerbox](https://github.com/strawlab/triggerbox)
hardware to synchronize the cameras. This is based on an Arduino
microcontroller.

On Ubuntu, it is important to add your user to the `dialout` group so that you
can access the Triggerbox. Do so like this:

```ignore
sudo adduser <username> dialout
```

### Trigger cables

TODO: write this and describe how to check everything is working.
