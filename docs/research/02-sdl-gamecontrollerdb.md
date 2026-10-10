# SDL GameControllerDB

Source: `mdqinc/SDL_GameControllerDB` at commit `c1d5289` (2026-10-02).
License: zlib (`LICENSE`, copyright Sam Lantinga). Compatible with
`MIT OR Apache-2.0`; keep the notice when importing.

## Content

`gamecontrollerdb.txt`: 2,291 lines, about 2,280 mappings. Per platform:
Windows 869, Linux 744, Mac OS X 325, Android 299, iOS 42.

One line per mapping:

```text
030000004c050000c405000000010000,PS4 Controller,a:b1,b:b2,back:b8,dpdown:h0.4,...,platform:Mac OS X,
```

- Field 1: SDL joystick GUID (see [01-sdl3.md](01-sdl3.md#joystick-guid)).
  It already encodes bus, VID, PID, version and the SDL backend.
- Field 2: name.
- Then `target:source` pairs. Targets are positional despite their Xbox
  names: `a` = south, `b` = east, `x` = west, `y` = north (the README asks
  for "the Xbox layout per the mapping guide").
- Sources: `bN` button, `aN` axis, `hN.M` hat N with bitmask M
  (1 up, 2 right, 4 down, 8 left). Modifiers: `+aN` / `-aN` half axis, `aN~`
  inverted axis; a target may also be prefixed with `+`/`-` (half-axis output,
  e.g. `dpright:+a0`). 204 lines use half axes, 41 use inversion.
- Usage of optional targets: `touchpad` 116, `misc1` 141, `paddle1/2` 87,
  `paddle3/4` 29, `guide` 1,123 lines.
- Extra fields understood by SDL (`SDL_gamepad.c`): `platform:`, `crc:`,
  `face:` (`abxy`, `axby`, `bayx`, `sony`), `hint:` (e.g.
  `hint:!SDL_GAMECONTROLLER_USE_BUTTON_LABELS:=1`), `sdk>=:` / `sdk<=:`
  (Android).

## What the indices mean

`bN`, `aN` and `hN` are SDL's **raw joystick indices on that platform's SDL
backend**, not HID usages. On Linux evdev, SDL numbers buttons from
`BTN_JOYSTICK` upward and then the codes below it, hats from `ABS_HAT*`
pairs, and the remaining `ABS_*` codes in code order (see
[01-sdl3.md](01-sdl3.md#linux-evdev-backend-details-needed-by-the-gamecontrollerdb-importer)).
Windows and macOS entries refer to DirectInput/RawInput or IOKit element
order.

## Contribution policy

The README says: "Please do not submit AI-generated comments or content in
issues or pull requests."

## Implications for anygamepad (D2)

- The importer converts each line into a profile with `source =
  "sdl-gamecontrollerdb"`, `verified = false` and the zlib notice.
- Match rules come from decoding the GUID (bus, VID, PID, version, driver
  signature) plus `platform`; the CRC16 field also needs the device name.
- To apply a mapping, our backend must enumerate raw elements in the same order
  as SDL's backend for that platform. Either reproduce SDL's ordering in
  our native providers (Linux rules above are simple) or store the
  order-independent source (evdev code, HID usage) when converting. The
  second option is the robust one; it needs the per-platform ordering
  rules at import time.
- Entries are per platform and per SDL backend; the browser is not a
  platform in GameControllerDB, so the web needs its own data (browser
  remapping) collected by the web tester.
- Improvements found through our data loop cannot be sent back as
  AI-generated pull requests; upstream contributions must be human-authored.
