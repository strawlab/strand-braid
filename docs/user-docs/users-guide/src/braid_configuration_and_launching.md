# Braid Configuration and Launching

## How to launch Braid

The central runtime of Braid, the `braid-run` executable, is launched from the
command line like so:

```ignore
braid run braid-config.toml
```

The `braid-config.toml` is the path of a Braid TOML configuration file.

## Braid TOML configuration files

The Braid configuration file, in the [TOML format](https://toml.io/), specifies
how Braid and multiple Strand Camera instances are launched. Any options not
specified result in default values being used. The defaults should be reasonable
and secure, allowing minimal configurations describing only specific aspects of
a particular setup.

The reference documentation for the `BraidConfig` type, which is automatically
deserialized from a `.toml` file:
[`braid_config_data::BraidConfig`](https://strawlab.org/strand-braid-api-docs/latest/braid_config_data/struct.BraidConfig.html).

Here is a minimal configuration for a 3 camera Braid setup:

```toml
{{#include ../../../../braid/simple.toml}}
```

Each camera `name` is computed from its vendor and serial number (for example
`Basler-22005677`). To discover the names of the connected cameras without
launching a camera, run:

```sh
strand-cam --list-cameras
```

This prints the available cameras (name, model, and serial) for the selected
`--camera-backend` (Basler Pylon by default) and exits. Use a printed name as
the `name` of a `[[cameras]]` entry above, or with `strand-cam --camera-name`.

## Camera synchronization (the `[trigger]` table)

For 3D tracking, all cameras must expose frames that were acquired at the same
instant so that Braid can combine their 2D detections. How this synchronization
is achieved is configured in the optional `[trigger]` table. The variant is
selected with the `trigger_type` key. If the `[trigger]` table is omitted, Braid
defaults to `FakeSync` (see below), which is **not** true synchronization and is
intended only for testing.

The reference documentation is
[`braid_types::TriggerType`](https://strawlab.org/strand-braid-api-docs/latest/braid_types/enum.TriggerType.html).

### `PtpSync` — GigE cameras synchronized over the network with PTP

This is the recommended method for GigE Vision cameras (such as Basler GigE
models) that support the Precision Time Protocol (PTP, IEEE 1588). The cameras
discipline their clocks to a PTP master clock running on the host PC (for
example via [`ptpd`](https://github.com/ptpd/ptpd)), and Braid programs each
camera to emit frames on a shared periodic schedule.

```toml
[trigger]
trigger_type = "PtpSync"
# The frame period in microseconds. 25000 µs = 25 ms = 40 fps.
periodic_signal_period_usec = 25000.0
```

The **camera frame rate is set by `periodic_signal_period_usec`** — it is the
interval between triggers, in microseconds. For example `25000.0` gives 40 fps,
`20000.0` gives 50 fps, and `10000.0` gives 100 fps.

> **Warning:** Do not set the period shorter than the camera exposure time
> (configured in the camera's `.pfs`/settings). If the exposure is longer than
> the trigger period, the camera cannot produce a frame for every trigger, and
> the effective frame rate will differ from the configured value and cameras may
> desynchronize. Set the exposure time below the period (e.g. for 40 fps / 25 ms,
> use an exposure well under 25 ms).

Setting up `ptpd` and the network (jumbo frames, the correct interface, the host
as PTP master) is an operating-system task performed once; see the
[Troubleshooting](./troubleshooting.md#ptp-synchronization-problems) page if
cameras fail to synchronize.

### `TriggerboxV1` — hardware trigger over USB

Cameras are triggered by a hardware pulse from a [Straw Lab
triggerbox](https://github.com/strawlab/triggerbox). This works with cameras
that have an external trigger input (including USB cameras) and provides
sub-millisecond timing.

```toml
[trigger]
trigger_type = "TriggerboxV1"
device_fname = "/dev/trig1"   # serial device of the triggerbox
framerate = 100.0             # frames per second
```

### `DeviceTimestamp` — rely on camera-provided timestamps

Cameras are synchronized using timestamps reported by the cameras themselves
(for example, cameras already disciplined to a common clock by external means).

```toml
[trigger]
trigger_type = "DeviceTimestamp"
```

### `FakeSync` — no real synchronization (testing only)

Braid pretends the cameras are synchronized at a fixed nominal frame rate. This
is useful for development and with emulated cameras, but must **not** be used for
real 3D tracking, because frames from different cameras are not actually
simultaneous. The Braid GUI displays a warning when `FakeSync` is in effect.

```toml
[trigger]
trigger_type = "FakeSync"
framerate = 95.0
```

## Inspecting the resolved configuration

To see the full configuration that Braid will use — including all default
values filled in for options not present in your `.toml` file — run:

```sh
braid-show-config <config.toml>
```

This prints the complete resolved configuration to stdout without launching
Braid. It is useful for verifying settings before a recording session and for
understanding what defaults are in effect.
