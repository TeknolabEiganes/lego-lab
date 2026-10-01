# LEGO Education Computer Science & AI hardware

Research notes for LEGO Lab's hardware backend. Last checked: October 2026.

## Sources

| Source | What it gives us |
| --- | --- |
| [LEGO Education Computer Science & AI](https://education.lego.com/en-us/lego-education-computer-science-and-ai/) | Product family overview |
| [Kit 6-8 (45522)](https://education.lego.com/en-us/products/lego-education-computer-science-and-ai/45522/) | Kit contents: Double Motor, Single Motor, Colour Sensor, Controller, 2 connection cards |
| [Teacher getting-started guide](https://teach.legoeducation.com/en-gb/computer-science/get-started) | 5+ kit: Single Motor + Colour Sensor only |
| [github.com/LEGO/LEGOEducation](https://github.com/LEGO/LEGOEducation) | **Official** Python API docs for this hardware (connect, per-device APIs, constants, function reference) |
| [pypi.org/project/legoeducation](https://pypi.org/project/legoeducation/) | **Official** Python package (v1.1.1, BUSL-1.1). Its source defines the BLE wire protocol |
| [LEGO Education Coding Canvas](https://code.legoeducation.com) | LEGO's own (browser) app; also does firmware updates |

The SPIKE Prime protocol docs (<https://lego.github.io/spike-prime-docs>) do **not**
apply. The new hardware uses the same GATT service UUID but different framing
and different messages.

## Hardware

There is **no hub and there are no ports**. Each element is a separate,
battery-powered Bluetooth LE device:

| Device | Capabilities (from the official API) |
| --- | --- |
| **Single Motor** | run / run for time / run for degrees / run to absolute or relative position, speed, duty cycle, acceleration, end state; reports position, absolute position, speed, power, state, gestures (wiggle, slow/fast turn) |
| **Double Motor** | two motors (left / right / both) with the same motor commands, plus "movement" commands (move, move for time/degrees, tank, turn for degrees using the IMU, steering); 6-axis IMU (yaw/pitch/roll, accelerometer, gyro, orientation) and gestures (tap, double tap, collision, shake, freefall) |
| **Colour Sensor** | detected LEGO colour, reflection, raw RGB, HSV |
| **Controller** | two levers: position in percent and as an angle |
| **All devices** | a button (pressed/released), a coloured button light with patterns, a beeper, battery level, USB power state |

There is **no distance sensor** in these kits, so LEGO Lab has no distance
block and the simulator does not fake one.

Colours the Colour Sensor detects (Coding Canvas numbering): red, yellow,
blue, teal, green, purple, white (and "no colour").

### Connection cards

The kits include coloured connection cards with a 4-digit serial number.
Devices advertise the card colour and serial, so in a classroom full of
hardware you pick "the Single Motor on the blue 3683 card" rather than
an anonymous Bluetooth address.

## How it talks to Linux

**Bluetooth LE only.** The documented interface is BLE GATT. The USB cable is
for charging; there is no documented USB data or serial interface.

### Advertising / discovery

* Service UUID `0000FD02-0000-1000-8000-00805F9B34FB` is advertised.
* Manufacturer data, company ID `0x0397` (LEGO), payload (after the company ID):
  `[product_group, product_device, card_colour, serial_lo, serial_hi, …]`
  * product ID = `group << 8 | device`: `512` Single Motor, `513` Double Motor,
    `514` Colour Sensor, `515` Controller
  * card colour uses the firmware colour numbering (see below)
  * card serial is a little-endian `u16` (0–9999); colour 0 and serial 0
    mean no card has been scanned onto the device yet

### GATT

| Characteristic | Direction |
| --- | --- |
| `0000FD02-0001-1000-8000-00805F9B34FB` | app → device, write without response |
| `0000FD02-0002-1000-8000-00805F9B34FB` | device → app, notifications |

### Framing

Unlike SPIKE Prime there is **no COBS encoding and no delimiter**. A message
is `[message_id: u8][payload…]`, little-endian. Several commands may be
concatenated into one write (the official package uses this for "batch"
commands, up to the device's `maxPacketSize`). The response to request id
`n` has id `n + 1`.

Motor result messages (`MOTOR_RUN_FOR_DEGREES_RESULT` etc.) are sent when the
movement **finishes**, with status `0` completed, `1` interrupted, `2` NACK.
They start with the motor bit mask, so responses can be matched per motor.

### Session

1. Connect, subscribe to notifications.
2. `InfoRequest (0x00)` → `InfoResponse (0x01)`: RPC version
   (u8, u8, u16), firmware version, bootloader version, `maxPacketSize: u16`,
   `productGroupDevice: u16`. The official package 1.1.x requires RPC
   major.minor `2.0`; 1.0.x required exactly `1.0.73`. The messages are the
   same in both.
3. `DeviceNotificationRequest (0x28, delay_ms: u16)` → response `0x29`.
   Delay 0 = off, otherwise 15–1000 ms. Then `DeviceNotification (0x3C)`
   messages stream sensor state.
4. Optional `ProgramFlowNotification (0x20, action: u8)`: 0 start, 1 stop.

### Commands (ids, payload)

| Id | Message | Payload |
| --- | --- | --- |
| 110 | LightColor | colour `i8`, pattern `u8`, intensity `u8` |
| 112 | PlayBeep | pattern `u8`, frequency `u16`, repetitions `u8` |
| 114 | StopSound | – |
| 120 | MotorResetRelativePosition | motor mask `u8`, position `i32` |
| 122 | MotorRun | motor mask `u8`, direction `u8` |
| 124 | MotorRunForDegrees | motor mask `u8`, degrees `i32`, direction `u8` |
| 126 | MotorRunForTime | motor mask `u8`, time ms `u32`, direction `u8` |
| 128 | MotorRunToAbsolutePosition | motor mask `u8`, position `u16`, direction `u8` |
| 130 | MotorRunToRelativePosition | motor mask `u8`, position `i32` |
| 132 | MotorSetDutyCycle | motor mask `u8`, duty `i16` (×100) |
| 138 | MotorStop | motor mask `u8` |
| 140 | MotorSetSpeed | motor mask `u8`, speed `i8` (−100…100) |
| 142 | MotorSetEndState | motor mask `u8`, end state `i8` |
| 144 | MotorSetAcceleration | motor mask `u8`, accel `u8`, decel `u8` |
| 150–177 | Movement* (Double Motor) | move, for time/degrees, tank, turn, stop, speed, end state, acceleration, steering |
| 190 / 192 | ImuSetYawFace / ImuResetYawAxis | face `u8` / value `i16` |

Each command has a result with id + 1 (`status: u8`; motor results are
`motor mask, status`).

Speed is **not** part of the run commands. The official package sends
`MotorSetSpeed` immediately followed by the run command in the same write.
Motor masks: Single Motor `1`; Double Motor left `1`, right `2`, both `3`.
Directions: `0` clockwise, `1` counter-clockwise, `2` shortest, `3` longest.

### Device notifications (`0x3C`)

Payload: `length: u16`, then a sequence of `[type: u8][fields]`:

| Type | Notification | Fields |
| --- | --- | --- |
| 0 | Info | battery `u8`, USB power `u8` |
| 1 | IMU | orientation `u8`, yaw face `u8`, yaw/pitch/roll `i16`, accel xyz `i16`, gyro xyz `i16` |
| 3 | Card | colour `i8`, serial `u16` |
| 4 | Button | state `u8` |
| 10 | Motor | mask `u8`, state `u8`, abs pos `u16`, power `i16`, speed `i8`, position `i32`, gesture `i8` |
| 12 | Colour sensor | colour `i8`, reflection `u8`, R/G/B `u16`, hue `u16`, saturation `u8`, value `u8` |
| 15 | Controller | left % `i8`, right % `i8`, left angle `i16`, right angle `i16` |
| 16 | IMU gesture | gesture `i8` |

### Colour numbering

The firmware numbers colours differently from the app:

| Colour | firmware | app |
| --- | --- | --- |
| none | −1 | 0 |
| black | 0 | (→ none) |
| magenta | 1 | 8 |
| purple | 2 | 6 |
| blue | 3 | 3 |
| azure | 4 | 10 |
| turquoise/teal | 5 | 4 |
| green | 6 | 5 |
| yellow | 7 | 2 |
| orange | 8 | 9 |
| red | 9 | 1 |
| white | 10 | 7 |

## The Rust backend

Implemented in `src/hardware/lego/`:

* `protocol.rs`: pure encode/decode of the messages above, unit-tested
  against the byte layouts of the official package.
* `mod.rs`: `btleplug` (BlueZ over D-Bus on Linux, Tokio based).
  * Scans 4 s, keeps devices advertising FD02 with LEGO manufacturer data.
    The devices panel lists them all; "Connect" in the toolbar picks the
    strongest-signal device of each kind.
  * Per device: connect, subscribe to notifications, `InfoRequest` (accepts
    RPC 1.0 and 2.0, refuses others with a firmware-update hint), then
    notifications every 50 ms.
  * Requests are matched to replies by `(response id, motor mask)`, FIFO.
    "For time" and "for degrees" resolve when the motor reports it has
    finished.
  * Sensor conditions read the latest notification values.
  * STOP writes `MotorStop` to every motor device without waiting for replies.
  * Disconnects (from BlueZ events or the notification stream ending) fail
    pending commands, so a running program stops.
  * Repeating an identical "run" within 500 ms is skipped so Forever loops
    don't flood the radio.

`lego-lab --scan` lists the devices in range.

`lego-lab --probe` connects, prints live readings and disconnects without
moving anything. `cargo run --example card_watch` prints connection-card
taps as they happen. `cargo run --example motor_test` runs a short, slow motor
test on a Double Motor.

### Verified on real hardware (Double Motor, firmware RPC 1.0.73, Oct 2026)

* Scan and advertisement: service FD02, manufacturer data `02 01 00 00 00`
  (product 513, no connection card scanned yet).
* Info, notifications (battery, both motor positions/speeds, button).
* Both motors for time (mask 3, the reply carries mask 3), left motor for
  degrees (turned 184° for 180°), run + stop of a single side.
* Motor notification `position` is cumulative degrees since power-on
  (e.g. 16148° after use) and survives reconnects; `absolutePosition` is the
  shaft angle 0–359°. `MotorResetRelativePosition` (mask 3, position 0)
  zeroes both sides without moving them. LEGO Lab does this on RUN.

* Connection cards: tapping a card on a connected Double Motor sends a Card
  notification (type 3) immediately; the card is also put in the
  advertisement (`02 01 01 ad 06` = pink 1709). Before any card is stored,
  colour and serial are 0. LEGO Lab shows the live card, falling back to the
  advertised one.
* The device's Bluetooth address changes between sessions (random/private
  addresses), so devices are identified by type and card, not address.

### Findings

* **BlueZ service filter:** on BlueZ 5.87, starting discovery with a
  UUID filter for FD02 silently doesn't start discovery (`stop_scan` then
  fails with "No discovery started"), so nothing is found. LEGO Lab scans
  unfiltered and filters on the service UUID and LEGO manufacturer data
  itself.
* **Protocol 1.0 vs 2.0:** devices on older firmware report RPC 1.0.73. The
  official package 1.0.6 speaks 1.0.73, 1.1.1 speaks 2.0.5, and their
  `rpc_message.py` files are identical apart from the version numbers.
  LEGO Lab accepts both.

## Licensing note

The `legoeducation` package is under the Business Source License 1.1. LEGO
Lab does not copy or depend on that code. The protocol facts above were read
from it so we can talk to the hardware; the Rust implementation will be
written independently.
