# Linux kernel gamepad drivers

Source: `torvalds/linux` master on 2026-10-10 (`drivers/input/joystick/xpad.c`,
`drivers/hid/hid-playstation.c`, `hid-nintendo.c`, `hid-microsoft.c`,
`hid-ids.h`, `Documentation/input/gamepad.rst`).

## Linux gamepad specification (`gamepad.rst`)

- Face buttons are **positional**: `BTN_SOUTH`, `BTN_EAST`, `BTN_NORTH`,
  `BTN_WEST` "regardless of the label" (`BTN_A` is an alias of `BTN_SOUTH`,
  `BTN_GAMEPAD` too).
- Sticks: `ABS_X/ABS_Y`, `ABS_RX/ABS_RY`. D-pad: `ABS_HAT0X/HAT0Y` or
  `BTN_DPAD_*`. Shoulders: `BTN_TL/BTN_TR`; triggers: `BTN_TL2/BTN_TR2` or
  `ABS_HAT2Y/ABS_HAT2X`. Profile switch: `ABS_PROFILE`.
- In practice drivers disagree on triggers: SDL's evdev backend documents
  `ABS_HAT2Y/HAT2X` (spec), `ABS_BRAKE/ABS_GAS` (Android-style Bluetooth pads)
  and `ABS_Z/ABS_RZ` (older Xbox and PlayStation).

## xpad (Xbox and compatibles, USB)

- 314 entries in `xpad_device[]`: Microsoft plus many third-party pads
  (each with a name and an `XTYPE_XBOX`, `XBOX360`, `XBOX360W`, `XBOXONE`).
- Buttons: `BTN_A/B/X/Y` (= south/east/west/north), `BTN_START`, `BTN_SELECT`,
  `BTN_THUMBL/R`, `BTN_TL/TR`, `BTN_MODE`; `KEY_RECORD` for the Series share
  button; `BTN_GRIPR/GRIPR2/GRIPL/GRIPL2` for Elite paddles.
- Sticks: range -32768..32767; **Y axes are inverted by the driver** (`~value`)
  so up is negative.
- Triggers: `ABS_Z/ABS_RZ`, range 0..255 (360) or 0..1023 (Xbox One).
- D-pad: `ABS_HAT0X/HAT0Y` or buttons; module parameters `dpad_to_buttons`,
  `triggers_to_buttons`, `sticks_to_null` change the layout **for unknown
  pads**, so the same pad can look different on two machines.

## hid-playstation (DualShock 4, DualSense, DualSense Edge)

- USB and Bluetooth, PS4 (`PS4_CONTROLLER`, `_2`, dongle) and PS5 (`_2` =
  Edge).
- **Three input devices per controller**: the gamepad, "Motion Sensors"
  (accelerometer on `ABS_X/Y/Z`, gyro on `ABS_RX/RY/RZ`) and "Touchpad"
  (multitouch `ABS_MT_POSITION_X/Y`, click as `BTN_LEFT`).
- Gamepad: positional buttons (Cross = `BTN_SOUTH`, Circle = `BTN_EAST`, ...),
  Create/Share = `BTN_SELECT`, Options = `BTN_START`, PS = `BTN_MODE`; sticks
  and triggers 0..255 (`ABS_X/Y/RX/RY`, `ABS_Z/RZ`); D-pad on `ABS_HAT0X/Y`;
  Edge Fn and back buttons on `BTN_TRIGGER_HAPPY1..`.
- The driver also handles the lightbar, player LEDs and rumble.

## hid-nintendo (Switch Pro, Joy-Cons, NSO controllers)

- Devices: Pro Controller (USB and Bluetooth), charging grip, Joy-Con L/R,
  SNES, Genesis/Mega Drive and N64 Online controllers, HORI wireless Switch
  pad.
- Positional mapping: Nintendo `A` = `BTN_EAST`, `B` = `BTN_SOUTH`,
  `X` = `BTN_NORTH`, `Y` = `BTN_WEST`. Licensed third-party Pro-style pads
  (controller type `0x06`, `lic_procon_button_mappings`) swap X and Y.
  Genesis and N64 tables map buttons differently again.
- The driver performs the handshake to enable full reports and creates a
  separate IMU input device.

## hid-microsoft (Xbox pads over Bluetooth)

- Adds force feedback (`MS_QUIRK_FF`) for Bluetooth Xbox controllers
  (models 1708, 1708 BLE, 1914, 1797, 1797 BLE); input comes from the generic
  HID mapping of the pad's report descriptor.
- The same table lists `USB_DEVICE_ID_8BITDO_SN30_PRO_PLUS` (`0x02e0`) under
  the **Microsoft vendor ID**: a third-party pad presenting itself as a
  Microsoft device, a concrete case for C8 (clone detection).

## Implications for anygamepad (C4, D3)

- On Linux, the kernel already normalizes the big families to positional
  evdev codes. The Linux provider should trust them and only remap generic HID
  pads.
- One physical controller can be several evdev nodes (gamepad + motion +
  touchpad, or gamepad + IMU). C4 must group them by their shared parent
  device (same HID device in sysfs) into one canonical device.
- Normalize ranges from `EVIOCGABS` (min, max, flat, fuzz) instead of
  assuming them, and record axis inversion per driver.
- The evdev view of a pad depends on the driver and on module parameters, so
  the OS driver name belongs in the identity (C2), as planned.
- Raw access (`hidraw`) works in parallel with the kernel driver, but sending
  handshakes or output reports while `hid-nintendo` or `hid-playstation` also
  does would conflict. Prefer the kernel driver when it is bound, and use our
  own protocol driver (C7) only when it is not, or read-only.
