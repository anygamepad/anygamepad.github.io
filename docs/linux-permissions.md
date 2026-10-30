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

## Virtual controllers (uinput, UHID)

`anygamepad_virtual::linux` (A5) creates the virtual Xbox 360 Controller
through `/dev/uinput` and the virtual HID pads (DualShock 4, DualSense,
Switch Pro) through `/dev/uhid`. Both nodes belong to root by default.
[`data/udev/70-anygamepad.rules`](../data/udev/70-anygamepad.rules) gives
the user at the seat access:

```sh
sudo cp data/udev/70-anygamepad.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
sudo modprobe uinput uhid
```

To load the modules at every boot, list them in
`/etc/modules-load.d/anygamepad.conf` (one per line). Sandboxed packages
(Flatpak, Snap) need these device nodes exposed as well (A9).

## Still to come

- Raw HID (`/dev/hidraw*`) for pads anygamepad drives itself (C7) needs a
  rule, since udev does not give `uaccess` to hidraw nodes by default.
