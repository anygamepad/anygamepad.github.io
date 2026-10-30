# anygamepad-virtual

Virtual controllers for the anygamepad desktop app: the controller types it
can emulate (Xbox 360, DualShock 4, DualSense, Switch Pro Controller), their
USB identities and HID report descriptors, and the sans-IO code that turns
canonical gamepad state into their input reports and the host's output
reports into canonical commands.

Part of [anygamepad](https://github.com/anygamepad/anygamepad.github.io).
Not usable yet: the project is in its first development phase, and the
platform outputs (uinput/UHID, ViGEmBus, `IOHIDUserDevice`) come later.
