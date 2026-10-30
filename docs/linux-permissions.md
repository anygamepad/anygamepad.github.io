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

## Still to come

- Raw HID (`/dev/hidraw*`) for pads anygamepad drives itself (C7) needs a
  rule, since udev does not give `uaccess` to hidraw nodes by default.
- Virtual controllers (`/dev/uinput`, `/dev/uhid`, A5) need rules too.

Both will be listed here with the tasks that need them.
