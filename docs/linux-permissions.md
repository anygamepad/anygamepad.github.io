# Linux permissions

What anygamepad needs on Linux, and the udev rules that grant it.

## Reading gamepads (evdev)

The Linux provider (`anygamepad::native::linux`, roadmap task C4) reads
`/dev/input/event*` nodes. Desktop distributions already let the user at
the seat read and write gamepad nodes: udev marks devices with joystick
buttons `ID_INPUT_JOYSTICK=1`, and systemd-logind gives the seat's user an
ACL on them (`uaccess`). Nothing to install there.

Without logind (some minimal or headless systems), add the user to the
group that owns the nodes (usually `input`):

```sh
sudo usermod -aG input "$USER"   # then log out and in again
```

The provider never opens keyboards or mice: it classifies nodes from sysfs
before opening them. It counts the gamepad nodes it cannot open
(`LinuxProvider::inaccessible`), so an app can point here.

## Exclusive access

`Access::Exclusive` grabs the nodes (`EVIOCGRAB`): other programs reading
them get no events while the app holds them. It needs no extra permission.

## Pads anygamepad drives itself (hidraw)

The raw HID provider (`anygamepad::native::hidraw`, C4 and C7) talks to
DualShock 4, DualSense, DualSense Edge and Switch Pro Controllers through
`/dev/hidraw*` when no kernel protocol driver drives them (they are bound
to `hid-generic`). Those nodes belong to root: udev gives the seat's user
no access to them by default.
[`data/udev/70-anygamepad.rules`](../data/udev/70-anygamepad.rules) gives
access to the hidraw nodes of these pads, and only of them (see below for
installing it). The provider counts the pads it cannot open
(`HidrawProvider::inaccessible`).

Opening a pad with `Access::Shared` reads a PlayStation pad's calibration,
which switches a Bluetooth pad to its full report until it is turned off:
generic HID readers (some games, browsers) then stop getting its input.
`Access::ObserveOnly` sends nothing.

## Lights (LED class)

Kernel drivers (`hid-playstation`, `hid-sony`, `hid-nintendo`) expose a
pad's light bar and player LEDs under `/sys/class/leds`, writable by root
only. The Linux provider offers them (`Output::Lightbar`,
`Output::PlayerLeds`) when it can write them. The same rules file makes
these LEDs writable when the pad connects; they only set lights.

## Virtual controllers (uinput, UHID)

`anygamepad_virtual::linux` (A5) creates the virtual Xbox 360 Controller
through `/dev/uinput` and the virtual HID pads (DualShock 4, DualSense,
Switch Pro) through `/dev/uhid`. Both nodes belong to root by default.
[`data/udev/70-anygamepad.rules`](../data/udev/70-anygamepad.rules) gives
the user at the seat access to them, to the pads' hidraw nodes and to
their LEDs:

```sh
sudo cp data/udev/70-anygamepad.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
sudo modprobe uinput uhid
```

To load the modules at every boot, list them in
`/etc/modules-load.d/anygamepad.conf` (one per line). Sandboxed packages
(Flatpak, Snap) need these device nodes exposed as well (A9).
