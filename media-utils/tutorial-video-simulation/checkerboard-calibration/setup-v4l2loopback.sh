#!/bin/bash
#
# One-time (per-boot) setup for checkerboard-calibration/record.sh's
# v4l2loopback dependency -- see ../README.md's "Checkerboard calibration
# and v4l2loopback" section. Needs sudo; not something record.sh does
# itself. The apt-get install persists across reboots, but the modprobe
# does not -- rerun this after every reboot.
#
# Tolerates a real failure mode seen on a multi-kernel machine (2026-07-20):
# apt-get install can return non-zero and leave the package
# "half-configured" if DKMS fails to build the module for a SECONDARY
# installed kernel (not the one currently running) -- e.g. a stale
# /var/crash/v4l2loopback-dkms.*.crash file blocking DKMS's own
# error-reporting step for that other kernel. That doesn't mean the module
# is unusable: what matters is whether it built for `uname -r`, checked via
# `dkms status` below, not apt's own exit code.

set -o nounset
set -o pipefail

CHECKERBOARD_LOOPBACK_LABEL="${CHECKERBOARD_LOOPBACK_LABEL:-checkerboard-cam}"

set +o errexit
sudo apt-get install -y v4l2loopback-dkms
APT_STATUS=$?
set -o errexit

RUNNING_KERNEL=$(uname -r)
MODULE_OK_FOR_RUNNING_KERNEL=0
if dkms status 2>/dev/null | grep -i v4l2loopback | grep -q "$RUNNING_KERNEL.*installed"; then
    MODULE_OK_FOR_RUNNING_KERNEL=1
fi

if [ "$APT_STATUS" -ne 0 ]; then
    if [ "$MODULE_OK_FOR_RUNNING_KERNEL" -ne 1 ]; then
        echo "=== apt-get install failed, and no working v4l2loopback module was found for the running kernel ($RUNNING_KERNEL). ===" >&2
        echo "Check: dkms status, and /var/lib/dkms/v4l2loopback/*/build/make.log" >&2
        exit 1
    fi
    echo "=== apt-get install returned an error, but the module IS built/installed for the running kernel ($RUNNING_KERNEL) -- continuing. ==="
    echo "This usually means DKMS failed to build for a DIFFERENT installed kernel (e.g. after a kernel"
    echo "update, before rebooting into it), often due to a stale crash report blocking DKMS's own error"
    echo "path. Attempting to clear that and let dpkg finish configuring the package:"
    CRASH_FILE=$(ls /var/crash/v4l2loopback-dkms.*.crash 2>/dev/null | head -1 || true)
    if [ -n "$CRASH_FILE" ]; then
        echo "  removing stale crash report: $CRASH_FILE"
        sudo rm -f "$CRASH_FILE"
    fi
    sudo dpkg --configure -a \
        || echo "  (dpkg --configure -a still failed -- package left half-configured, but the running kernel's module works; safe to ignore for this script's purposes)"
fi

sudo modprobe v4l2loopback video_nr=9 card_label="$CHECKERBOARD_LOOPBACK_LABEL" exclusive_caps=1

echo "=== Done. Verifying device: ==="
FOUND=0
for f in /sys/class/video4linux/video*/name; do
    [ -r "$f" ] || continue
    if [ "$(cat "$f")" = "$CHECKERBOARD_LOOPBACK_LABEL" ]; then
        echo "OK: $(dirname "$f" | xargs basename) -> $CHECKERBOARD_LOOPBACK_LABEL"
        FOUND=1
    fi
done
[ "$FOUND" -eq 1 ] || {
    echo "ERROR: modprobe succeeded but no device found with card_label=$CHECKERBOARD_LOOPBACK_LABEL" >&2
    exit 1
}
