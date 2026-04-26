# Troubleshooting

## Synchronization problems

Braid uses the [Straw Lab Triggerbox](https://github.com/strawlab/triggerbox)
to send a hardware trigger signal to all cameras simultaneously. If cameras are
not synchronized, check the following:

**Permission to access the Triggerbox serial port.** On Ubuntu, the Triggerbox
appears as a serial device (e.g. `/dev/ttyUSB0`). Your user must be in the
`dialout` group to access it:

```ignore
sudo adduser <username> dialout
```

After adding yourself to the group, log out and back in for the change to take
effect. You can verify group membership with the `groups` command.

**Triggerbox connected and detected.** Confirm the Triggerbox is plugged in and
that the device file appears under `/dev/ttyUSB*` or `/dev/ttyACM*`. The Braid
log output at startup will indicate whether the Triggerbox was found.

**Trigger cables.** Each camera must be connected to the Triggerbox with a
trigger cable. Verify that every cable is seated properly at both ends.

**Frame rate consistency.** Ensure that every camera is configured to the same
frame rate. Mismatched frame rates will cause synchronization failures even when
the hardware trigger is working correctly.

## PTP synchronization problems

When using GigE cameras synchronized over a network via PTP (Precision Time
Protocol), synchronization failures typically appear in the Braid log as
repeated warnings such as:

```text
launch time precedes device timestamp. Is time running backwards?
```

or as Braid reporting that cameras are not synchronizing.

**PTP daemon not running or misconfigured.** The PTP daemon (e.g. `ptpd`) must
be running with the host PC set as the PTP master clock. Check its status with:

```sh
systemctl status ptpd
```

A healthy output will include a `Started ptpd.service` log entry. If not, check
that the daemon is configured correctly (the `ptpengine:preset=masteronly` and
correct `ptpengine:interface` settings) and restart it:

```sh
systemctl restart ptpd
```

**Wrong network interface.** The PTP daemon must be bound to the same network
interface that the cameras are connected to. Use `ip link | grep "state UP"` to
identify the correct interface name and confirm it matches the
`ptpengine:interface` setting in your PTP configuration file.

**Jumbo frames not enabled.** GigE cameras typically require jumbo frames (MTU
9000) on both the PC network interface and the switch. Verify the MTU is set to
9000 on the PC's network connection and that jumbo frames are enabled in the
switch settings.

**Camera clocks drifting on first launch.** On the first launch after cameras
have been powered on it can take several seconds for PTP to bring all camera
clocks into agreement. The "Is time running backwards?" warning will stop
appearing once synchronization is established. If it persists for more than
30 seconds, check the items above.

## AprilTag detection drops frames during calibration

AprilTag detection is computationally intensive. If you see frame-drop errors
while recording AprilTag detections for calibration, try one of the following:

- Run Strand Camera as a standalone instance (not via Braid) for the calibration step.
- Reduce the camera frame rate to around 10 FPS for this step only.

## Any other problem or question

Please [report any issues you
face](https://github.com/strawlab/strand-braid/issues) or [ask any questions you
may have](https://groups.google.com/forum/#!forum/multicams).
