# LEGO Lab

A native Linux desktop app, written in Rust, for visually programming the
**LEGO Education Computer Science & AI** kits. Children drag blocks onto a
canvas, snap them together and press **RUN**.

No browser, HTML, JavaScript, Electron, Tauri, WebView or Blockly. The UI is
[egui](https://github.com/emilk/egui) via `eframe`.

> **Status:** Milestones 1–7. The editor, control flow (Repeat, Forever,
> If, If/Else, Wait Until), sensor conditions, executor, LEGO simulator and
> a Bluetooth backend for the real hardware. The protocol is documented in
> [`docs/lego-hardware.md`](docs/lego-hardware.md).

## Build and run on Arch Linux

```bash
# 1. Toolchain and build tools (clang builds the webcam bindings, alsa-lib is for sound)
sudo pacman -S --needed rust base-devel pkgconf clang alsa-lib

# 2. Graphics and windowing (Wayland and X11)
sudo pacman -S --needed libxkbcommon wayland libx11 libxcursor libxrandr libxi \
    vulkan-icd-loader mesa
#    and the Vulkan driver for your GPU, e.g. one of:
#    sudo pacman -S vulkan-radeon      # AMD
#    sudo pacman -S vulkan-intel       # Intel
#    sudo pacman -S nvidia-utils       # NVIDIA

# 3. Open/Save dialogs use the XDG desktop portal
sudo pacman -S --needed xdg-desktop-portal xdg-desktop-portal-gtk

# 4. Bluetooth (for real LEGO hardware)
sudo pacman -S --needed bluez bluez-utils dbus
sudo systemctl enable --now bluetooth

# 5. Build and run
cd lego-lab
cargo run --release
```

The first build takes a few minutes (the GPU stack is large). Later builds
are quick.

You can also open a project directly:

```bash
cargo run --release -- path/to/my-robot.legolab
```

Run the tests:

```bash
cargo test
```

## Using it

1. The app starts in **Bluetooth** mode, scans for LEGO devices and lists them
   in the Devices pane (see *Real LEGO hardware* below). It reopens the last
   project you opened or saved; on first launch, or if that file is missing,
   it starts with an empty canvas. A project path passed on the command line
   takes precedence.
2. Connect a device, then press **▶ RUN** (or F5). The running block glows.
   No hardware at hand? Start with `cargo run --release -- --simulator`, or
   pick **Simulator** in the toolbar.
3. Press **■ STOP** (or Esc) at any time. It cancels the program and stops
   every motor.

Editing:

| Action | How |
| --- | --- |
| Add a block | drag it from the palette onto the canvas |
| Connect blocks | drop a block near the bottom of another; it snaps |
| Move / reorder / disconnect | drag a block; the blocks below come with it |
| Edit a parameter | click or drag the white fields inside a block |
| Select | click a block |
| Copy / paste selected block | Ctrl+C / Ctrl+V (Cmd+C / Cmd+V on macOS) |
| Delete | select + Delete/Backspace, right-click → Delete, or drag onto the palette |
| Duplicate | right-click → Duplicate |
| Toggle Blocks / Devices panes | Ctrl+1 / Ctrl+2; Ctrl+F toggles both (Cmd on macOS) |
| Show the Camera pane | Ctrl+3, or the 📷 Camera tab in the right pane |
| Pan | drag the empty canvas, or scroll |
| Zoom | Ctrl + scroll, pinch, or the − / % / + buttons (% resets) |
| Files | ☰ File menu, Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+S |

Only stacks that start with **When Run** run. Several When Run stacks run at
the same time.

### Blocks

| Category | Blocks |
| --- | --- |
| Events | When Run |
| Motors | Run Motor, Stop Motor, Motor for Time, Motor Rotations, Set Acceleration, Reset Position |
| Sounds | Beep, Play Tune |
| Control | Wait, Repeat, Forever, If, If / Else, Wait Until |
| Sensors | Colour is, Reflection, Button pressed, Motor position, Controller lever, Camera sees, Body points touch, Body distance, Body angle, Body position |

**Motor position** is how far a motor has turned, in degrees, counted
from 0 when you press RUN (so 720° = two turns). *Reset Position* sets it
back to 0 during a program. The devices panel also shows each motor's
**angle** (0–359°, where the shaft points).

Repeat, Forever, If and If / Else are C-shaped: drop blocks into their
mouths. Nothing can be attached below Forever.

Sensors are conditions. Drop one onto the teal area of an If, If / Else or
Wait Until block to set its condition, or drop it on the empty canvas to get
a *Wait Until* block. You can also change the condition with the dropdowns
inside the block.

Motor blocks and sensor conditions have a card-group dot at the top right. A
solid white dot means **No card**: target all connected devices of that kind
without a connection card. **All cards** targets card-assigned devices of that
kind regardless of colour; choosing a specific card colour targets only that
group. Motors in a selected group all receive the command; a sensor condition
is true if any matching device's latest reading satisfies it. Card colour
identifies the device group—it is separate from the colour a Colour Sensor
detects.

The **Beep** sound block plays one of the devices' built-in patterns (single,
double, triple, or up & down) on a note from C4 to C7, 1–10 times, on every
connected device in its selected card group. **Play Tune** plays a short
built-in melody the same way, one beep per note. STOP also silences beeps.

Sound blocks can also play on **this computer**: tap the dot at the top
right of a Beep or Play Tune block and choose *LEGO devices*, *This
computer* or *Devices + computer*. Sound on the computer has a **volume**
(0–100%, 100% by default); the LEGO devices have no volume setting in their
protocol, but their beepers are loudest at high notes, so new Beep blocks
start on C7. A program whose sounds only play on the computer runs without
any LEGO devices connected.

### Camera

**Camera sees** uses the computer's webcam as a sensor, like *Pose
Classification* in LEGO's Coding Canvas. It finds a person's body points
(the same MoveNet model Coding Canvas uses, bundled and run on this computer;
pictures never leave it) and is true when the camera sees:

* **a person** / **no person**
* **arms down**, **left arm up**, **right arm up** or **both arms up** (the
  person's own left and right; only one of these is true at a time)
* one of **your own poses**

The **body sensing** conditions are Coding Canvas's *Body Sensing* blocks.
They work with any two of the 17 body points (nose, eyes, ears, shoulders,
elbows, wrists, hips, knees, ankles; the wrists stand for the hands):

* **Body points touch**: e.g. *left wrist touches right wrist* (hands
  together) or *right wrist touches nose*.
* **Body distance**: how far apart two points are, e.g. hands > 50.
* **Body angle**: which way the second point is from the first: 0° right,
  90° up, 180° left, 270° down. *Left shoulder → right shoulder* is about 0°
  when you stand straight and changes when you lean.
* **Body position**: x or y of a point, e.g. *y of nose < 0* (ducking).

They use Coding Canvas's units: 0 is the middle of the picture, y is 45 at
the top and −45 at the bottom, x is about −60 (left) to 60 (right) on a 4:3
webcam, and points touch when they are less than 20 apart. Left and right
are as in the mirrored preview. The **Measure** card in the Camera pane shows
the live values for two points you pick, to help choose the numbers.

Open the **📷 Camera** tab in the right pane (Ctrl+3) and press **Turn on**
to see the picture, mirrored, with the body points drawn on it. To teach the
camera your own poses, press **＋ Add pose**, name it, and hold **● Record**
while you do the pose (move a little for varied examples; 20–40 is plenty).
Teach at least two poses, for example "star" and "standing", so it can tell
them apart. Trained poses are saved in the project file and appear in the
*Camera sees* dropdown.

If a program uses the camera, RUN switches it on (and off again when the
program ends, unless the Camera pane is open). While it is starting, camera
conditions are false; if there is no camera, the program stops with a
message. "📷 Camera on" shows in the status bar whenever it is on.

### Example

`examples/colour-motor.legolab` is the program from the plan:

```text
WHEN RUN
FOREVER
    IF colour is red
        Single motor → 100%
    ELSE
        Stop single motor
```

```bash
cargo run --release -- --simulator examples/colour-motor.legolab
```

With `--simulator`, press RUN, then change *Simulated colour* in the simulator panel: red starts
the motor, any other colour stops it. A second script drives the Double
Motor four times, each time waiting for you to hold the Controller's button.

### Real LEGO hardware

1. Switch on your LEGO devices. Their light blinks when they are ready to
   connect. If they have never been used, update their firmware once at
   <https://code.legoeducation.com>.
2. Start LEGO Lab (it starts in **Bluetooth** mode). The **Devices**
   panel scans and lists every LEGO device nearby with its connection card
   (colour and number). Press **🔍 Scan** to look again.
3. Press **Connect** next to each device you want. You can connect multiple
    devices of the same type; blocks route to them by motor/sensor type and
    card-colour group. Connected devices show their card, battery and live
    readings.

    The **Card colour** filter narrows the device list. **Connect all** (or
    **Connect filtered**) connects every device from the latest scan, either
    unfiltered or matching the selected card colour. In a busy classroom, use
    the individual Connect buttons to choose devices.
4. Press RUN as usual.

From a terminal:

```bash
cargo run --release -- --scan          # list LEGO devices in range
cargo run --release -- --probe         # connect, show live readings (no movement)
cargo run --release --example motor_test   # short slow test on a Double Motor
```

Tested with a Double Motor on firmware 1.0.73 (protocol 1.0); protocol 2.0
devices use the same messages and are accepted too.

If a device is switched off or goes out of range during a program, the
program stops and every motor is told to stop.

### Simulator

The simulator panel simulates every device in the kits: Single Motor, Double
Motor (left/right), Colour Sensor (pick a colour, set reflection), Controller
(two levers) and each device's button (hold to press). You can switch
devices on and off, assign each simulated device a card group, and see how the
app reacts to missing hardware. There is also a timestamped command log.

## The hardware (short version)

These kits have **no hub and no ports**. Each element (Single Motor, Double
Motor, Colour Sensor, Controller) is its own Bluetooth LE device, so motor
blocks pick a motor (Single motor, Double motor left/right/both) rather than
a port letter. There is **no distance sensor** in the kits. Full details and
sources are in [`docs/lego-hardware.md`](docs/lego-hardware.md).

## Architecture

```text
egui UI (src/ui)  ──edits──▶  visual program (src/visual.rs)
                                   │  program::compiler
                                   ▼
                       Program / Command (src/program)
                                   │
                                   ▼
                     Executor (src/executor, Tokio task)
                                   │  LegoDevice trait
                         ┌─────────┴──────────┐
                  MockDevice (mock.rs)   LegoBleDevice (lego/, btleplug)
```

* `src/visual.rs`: what's on the canvas: stacks, blocks, positions, raw parameters.
* `src/program/`: the typed, validated `Command` and `Condition` model and the compiler.
* `src/executor/`: async interpreter with cancellation and highlight events.
* `src/camera/`: webcam capture (`v4l`), the MoveNet pose model (`tract`,
  `assets/movenet-singlepose-lightning.onnx`, Apache-2.0), built-in poses and
  nearest-neighbour matching of trained poses.
* `src/hardware/`: the `LegoDevice` trait; `mock.rs` simulator;
  `lego/protocol.rs` LEGO wire format (pure, unit-tested); `lego/mod.rs`
  Bluetooth connections via `btleplug`.
* `src/project/`: versioned JSON project files.
* `src/ui/`: toolbar, palette, canvas, block drawing, status bar, simulator panel.

The UI never touches hardware. `LegoDevice` methods take `&self` (with
interior mutability) rather than living behind one `Mutex`, so a long
"motor for 10 seconds" can never block STOP.

### Safety

* STOP cancels the program and sends *stop all motors*, without waiting for
  the program.
* If a command fails (hardware error, a device disconnects, a task crashes),
  the run is stopped and all motors are stopped.
* Closing the window stops all motors and disconnects.
* Speeds are a `Speed` type clamped to 0–100%. Times, rotations and positions
  are clamped too.
* Project files are parsed into strict types (unknown block types are
  rejected) and then sanitised (ranges clamped, duplicate ids fixed, structure
  repaired) before anything can run.

## Project file format

`*.legolab` files are JSON:

```json
{
  "version": 1,
  "name": "My Robot",
  "stacks": [
    {
      "pos": [80.0, 60.0],
      "blocks": [
        { "id": 1, "type": "when_run" },
        { "id": 2, "type": "run_motor", "motor": "Single", "group": "NoCard", "direction": "Clockwise", "speed": 50 },
        { "id": 3, "type": "wait", "seconds": 2.0 },
        { "id": 4, "type": "stop_motor", "motor": "Single", "group": "NoCard" }
      ]
    }
  ]
}
```

Trained camera poses are stored in a top-level `poses` list (name plus the
recorded body points of each example); *Camera sees* conditions refer to
them by position, e.g. `{ "sensor": "camera", "pose": { "Custom": 0 } }`.

Connections are implied by order within a stack; blocks inside a C-block are
in its `body` (and `else_body` for If / Else). Projects made before card-group
selection was added omit `group`; those blocks load as `NoCard`.

## Roadmap

* Double Motor movement blocks (drive, turn using the IMU) and gesture
  conditions (tap, shake).
* Light and beep blocks (every device has a coloured light and a beeper).
* Choosing specific devices by connection card (for classrooms with many kits).
