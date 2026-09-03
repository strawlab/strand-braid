# The LED box

The LED box is a small microcontroller device from the Straw Lab that switches
up to four LED channels on and off under software control. It is typically used
for optogenetic stimulation or for triggering a visual cue during an experiment.

There are three ways to drive it:

* **`led-box-standalone`** — a small GUI application with one button per
  channel. This is the easiest way to check that the hardware works.
* **Strand Camera** — start Strand Camera with `--led-box <PORT>` and the
  channel controls appear in the browser interface alongside the camera
  controls. (Braid's `.toml` configuration has no LED box setting; the box is
  configured per Strand Camera process.)
* **Your own script** — the box is an ordinary serial device speaking a simple
  line-based JSON protocol, so it can be driven directly from Python. This is
  described below.

## The serial protocol

The LED box appears as a serial port: `COM3`, `COM4`, … on Windows,
`/dev/ttyACM0` or `/dev/ttyUSB0` on Linux. Communication is **newline-delimited
JSON** (one complete JSON value per line, terminated by `\n`) at **230400 baud,
8N1**. Every message you send is answered by exactly one message from the box.

| Constant | Value |
| :--- | :--- |
| Baud rate | `230400` |
| Protocol version (`COMM_VERSION`) | `3` |
| Maximum intensity (`MAX_INTENSITY`) | `16000` |

Messages to the device:

| Message | On the wire |
| :--- | :--- |
| Ask for the firmware protocol version | `"VersionRequest"` |
| Set the state of all four channels | `{"DeviceState": {...}}` |
| Round-trip latency check | `{"EchoRequest8": [b0, …, b7]}` |

Messages from the device:

| Message | On the wire |
| :--- | :--- |
| Reply to a version request | `{"VersionResponse": 3}` |
| Acknowledgement of a state change | `"StateWasSet"` |
| Reply to an echo request | `{"EchoResponse8": [b0, …, b7]}` |

A `DeviceState` message always carries **all four channels** — there is no
partial update, so to change one channel you resend the full state with the
other three left as they were. Each channel has a number (1–4), an `on_state`
of either `"Off"` or `"ConstantOn"`, and an `intensity` between `0` and
`16000`, which sets the PWM duty cycle. Turning channel 1 fully on and leaving
the rest off looks like this (shown wrapped here, but sent as a single line):

```json
{"DeviceState":{
  "ch1":{"num":1,"on_state":"ConstantOn","intensity":16000},
  "ch2":{"num":2,"on_state":"Off","intensity":16000},
  "ch3":{"num":3,"on_state":"Off","intensity":16000},
  "ch4":{"num":4,"on_state":"Off","intensity":16000}}}
```

## Finding the serial port

The example script below can list the serial ports it can see:

```sh
pip install pyserial
python led-box-blink.py --list
```

On Windows the LED box will be one of the `COM` ports; the Raspberry Pi Pico
build identifies itself as `USB VID:PID=16C0:27DD`, while the Nucleo build is
bridged by the board's on-board ST-Link and shows up as `USB
VID:PID=0483:374B`. The script guesses the port when the choice is
unambiguous, and otherwise asks you to name it with `--device`. The same port
name is what you pass to Strand Camera as `--led-box`.

> On Linux your user must have permission to open the serial port. If you get a
> permission error, add yourself to the `dialout` group and log back in. See
> [Troubleshooting](./troubleshooting.md).

## Demo: blinking an LED at 1 Hz

[`led-box-blink.py`](https://github.com/strawlab/strand-braid/blob/main/docs/user-docs/scripts/led-box-blink.py)
opens the LED box, checks the firmware protocol version, and then switches
channel 1 on and off once per second until you press Ctrl-C. It is the scripted
equivalent of clicking the channel 1 button in `led-box-standalone` twice a
second. Run it like so:

```sh
pip install pyserial
python led-box-blink.py --device COM3
```

Omit `--device` to let the script guess the port, and pass `--period` to change
the blink period (`--period 0.5` gives 2 Hz).

Stripped of its argument parsing and error checking, the whole interaction is
just this:

```python
import json, time, serial

def state(ch1_on):
    def ch(num, on):
        return {"num": num, "on_state": "ConstantOn" if on else "Off", "intensity": 16000}
    return {"DeviceState": {"ch1": ch(1, ch1_on), "ch2": ch(2, False),
                            "ch3": ch(3, False), "ch4": ch(4, False)}}

with serial.Serial("COM3", 230400, timeout=1.0) as ser:
    while True:
        for on in (True, False):
            ser.write(json.dumps(state(on)).encode() + b"\n")
            ser.readline()  # the box replies "StateWasSet"
            time.sleep(0.5)
```

Reading the `"StateWasSet"` reply after each write is not strictly required,
but if you never read from the port the incoming replies accumulate in the
operating system's buffer. Reading them also confirms the box is alive: a
`readline()` that returns empty means the write did not get through.

To dim an LED rather than switching it, lower `intensity` (for example `1600`
for roughly 10% duty cycle) and leave `on_state` at `"ConstantOn"`. The LED
drive is PWM at a fixed frequency of about 500 Hz, so intensity is not
appropriate for stimuli that must be modulated faster than that; use
`on_state` for precise on/off timing instead.
