#!/usr/bin/env python
"""Blink LED channel 1 of a Strand Camera LED box at 1 Hz.

This is the scripted equivalent of connecting with the `led-box-standalone` GUI
and clicking the channel 1 button on and off once per second.

The LED box speaks newline-delimited JSON over a serial port, so `pyserial` is
the only dependency:

    pip install pyserial

Usage:

    python led-box-blink.py --list             # show the serial ports found
    python led-box-blink.py                    # auto-pick a port, then blink
    python led-box-blink.py --device COM3      # use a specific port (Windows)
    python led-box-blink.py --device /dev/ttyACM0   # (Linux)
"""

import argparse
import json
import sys
import time

import serial  # pip install pyserial
import serial.tools.list_ports

# These constants mirror the `strand-led-box-comms` crate. `COMM_VERSION` must
# match the firmware running on the box.
BAUD_RATE = 230_400
COMM_VERSION = 3
MAX_INTENSITY = 16000

# USB vendor/product IDs of the two supported LED box builds: the Raspberry Pi
# Pico firmware, which presents its own USB serial device, and the Nucleo
# firmware, which talks over a UART bridged by the board's on-board ST-Link.
# The ST-Link IDs are shared by every Nucleo board, so a match there is a hint
# rather than proof; pass --device if you have other ST-Link boards attached.
LED_BOX_USB_IDS = {
    (0x16C0, 0x27DD): "Raspberry Pi Pico LED box",
    (0x0483, 0x374B): "ST-Link virtual COM port (Nucleo LED box)",
}


def device_state(ch1_on, intensity=MAX_INTENSITY):
    """Build a `ToDevice::DeviceState` message.

    The device has no notion of a partial update: every message carries the
    complete state of all four channels. Here channels 2-4 are left off.
    """

    def channel(num, on):
        return {
            "num": num,
            "on_state": "ConstantOn" if on else "Off",
            "intensity": intensity,
        }

    return {
        "DeviceState": {
            "ch1": channel(1, ch1_on),
            "ch2": channel(2, False),
            "ch3": channel(3, False),
            "ch4": channel(4, False),
        }
    }


def send(ser, msg):
    """Serialize `msg` as one JSON line and write it to the device."""
    # Compact separators keep each line comfortably inside the device's
    # receive buffer.
    ser.write(json.dumps(msg, separators=(",", ":")).encode("utf-8") + b"\n")


def receive(ser):
    """Read one JSON line from the device. Returns None if nothing arrived."""
    line = ser.readline().strip()
    return json.loads(line) if line else None


def list_ports():
    return list(serial.tools.list_ports.comports())


def pick_default_port(ports):
    """Guess which serial port is the LED box.

    Prefers a port whose USB IDs match one of the known LED box builds;
    otherwise falls back to the sole available port. Returns None when the
    guess would be ambiguous, in which case pass --device explicitly.
    """
    matching = [p for p in ports if (p.vid, p.pid) in LED_BOX_USB_IDS]
    if len(matching) == 1:
        return matching[0].device
    if len(ports) == 1:
        return ports[0].device
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--list", action="store_true", help="list the serial ports and exit"
    )
    parser.add_argument(
        "--device", help="serial port of the LED box (e.g. COM3 or /dev/ttyACM0)"
    )
    parser.add_argument(
        "--period",
        type=float,
        default=1.0,
        help="seconds per on/off cycle (default: %(default)s, i.e. 1 Hz)",
    )
    args = parser.parse_args()

    ports = list_ports()
    device = args.device or pick_default_port(ports)

    if args.list or device is None:
        if not ports:
            print("No serial ports found.")
        for port in ports:
            note = LED_BOX_USB_IDS.get((port.vid, port.pid), "")
            print(f"{port.device}\t{port.description}\t{port.hwid}\t{note}")
        if args.list:
            return 0
        print("\nCould not guess which port is the LED box. Pass --device.")
        return 1

    print(f"opening {device}")

    with serial.Serial(device, BAUD_RATE, timeout=1.0) as ser:
        # Give a USB serial device a moment to settle after the port is opened,
        # then drop anything the device sent before we were listening.
        time.sleep(0.1)
        ser.reset_input_buffer()

        # Check the firmware speaks the protocol version this script assumes.
        send(ser, "VersionRequest")
        reply = receive(ser)
        if reply != {"VersionResponse": COMM_VERSION}:
            print(
                f"Unexpected reply to version request: {reply!r}. "
                f"Expected {{'VersionResponse': {COMM_VERSION}}}. "
                "Is the firmware up to date?",
                file=sys.stderr,
            )
            return 1
        print(f"connected to firmware version {COMM_VERSION}")

        print("blinking channel 1 -- press Ctrl-C to stop")
        try:
            while True:
                for on in (True, False):
                    send(ser, device_state(on))
                    receive(ser)  # the device answers "StateWasSet"
                    time.sleep(args.period / 2.0)
        except KeyboardInterrupt:
            print("\nturning LED off")
            send(ser, device_state(False))
            ser.flush()

    return 0


if __name__ == "__main__":
    sys.exit(main())
