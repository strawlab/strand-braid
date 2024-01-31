# Installation

## Software installation

Download releases from [our releases
page](https://github.com/strawlab/strand-braid/releases)

## Hardware installation

### Cameras

Currently only Basler cameras are supported. We use Basler's Pylon library to
access the cameras.

Support for other cameras is planned.

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
