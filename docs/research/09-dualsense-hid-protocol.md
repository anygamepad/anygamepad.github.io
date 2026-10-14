# DualSense and DualShock 4 HID protocol (WebHID)

Source: `daidr/dualsense-tester` at commit `62dce17` (2026-09-25), a WebHID
tester for DualShock 4, DualSense and DualSense Edge (Vue, Chromium only).
License: MIT. Files: `src/utils/dualsense/ds.util.ts`, `crc32.util.ts`,
`microphoneProtocol.ts`, `src/router/*/_utils/offset.util.ts`,
`src/router/*/views/_OutputPanel/outputStruct.ts`, `src/store/dualsense.ts`.

Facts below come from that code. None was checked on a real device in this
session. Points marked *(hid-playstation)* come from the Linux driver (note
05) and were not re-read here; check both in C7.

## Devices

| Pad | VID | PID |
|-----|-----|-----|
| DualShock 4 v1 | `054c` | `05c4` |
| DualShock 4 v2 | `054c` | `09cc` |
| DualSense | `054c` | `0ce6` |
| DualSense Edge | `054c` | `0df2` |

WebHID filter: these VID/PID with usage page `0x01` (Generic Desktop), usage
`0x05` (Game Pad).

## Transport from the report structure

WebHID does not say whether a device is on USB or Bluetooth. The tester
infers it from the Game Pad collection: the largest input report, summing
`reportSize * reportCount` over its items, is 504 bits on USB and 616 bits
on Bluetooth (DualSense). For DualShock 4, 504 bits means USB and anything
else Bluetooth. Other values mean "unknown", and the tester ignores the
device.

## Input reports

`HIDInputReportEvent.data` excludes the report ID. DualSense: report `0x01`
on USB; `0x31` on Bluetooth, where every offset is one byte later (a leading
sequence/tag byte) and the last 4 bytes are a CRC32 (offset 73). DualShock 4:
`0x01` on USB, `0x11` on Bluetooth with offsets two bytes later.

DualSense, USB offsets:

| Offset | Size | Field |
|-------:|-----:|-------|
| 0-3 | 1 each | Left X, left Y, right X, right Y (0-255, centre 128) |
| 4, 5 | 1 each | L2, R2 analog (0-255) |
| 6 | 1 | Sequence number |
| 7 | 1 | Bits 0-3: D-pad hat (0 = up, then clockwise to 7 = up-left; other values = released); bits 4-7: Square, Cross, Circle, Triangle |
| 8 | 1 | Bit 0: L1, 1: R1, 4: Create, 5: Options, 6: L3, 7: R3 (bits 2-3: L2/R2 digital *(hid-playstation)*) |
| 9 | 1 | Bit 0: PS, 1: touchpad click, 2: mute |
| 15-20 | 3 × i16 LE | Gyroscope |
| 21-26 | 3 × i16 LE | Accelerometer |
| 27 | u32 LE | Sensor timestamp |
| 32-39 | 2 × 4 | Touch points: byte 0 bit 7 set = not touching, bits 0-6 = id; X 12 bits (0-1919), Y 12 bits (0-1079) |
| 52 | 1 | Bits 0-3: battery 0-10; bits 4-7: 0 discharging, 1 charging, 2 full, 10/11 voltage/temperature fault, 15 error |
| 53 | 1 | Bit 0: headphones plugged; bit 1: microphone plugged |

DualSense Edge: offset 48 is the active profile and 49 the trigger level,
where the DualSense has a device timestamp.

On Bluetooth, report `0x31` also carries microphone audio. The low nibble of
its first byte is the payload type: 1 = controller state, 2 = audio (Opus).
A parser must check it before reading a `0x31` report as state.

## Output reports

DualSense, USB: report `0x02`, 47 bytes: two valid-flag bytes, right
(high-frequency) then left (low-frequency) rumble motor, headphone, speaker
and microphone volume, audio control, mute LED, power save, right trigger
effect (mode + 10 parameters), left trigger effect (mode + 10 parameters), 4
reserved bytes, haptic volume, audio control 2, valid flag 2, 2 reserved
bytes, lightbar setup, LED brightness, player LEDs, lightbar R, G, B.

DualSense, Bluetooth: report `0x31`, 77 bytes: byte 0 = 4-bit sequence
number in the high nibble, byte 1 = `0x10` (tag), then the 47 USB bytes,
then the CRC32 in the last 4 bytes.

DualShock 4: report `0x05` on USB, `0x11` on Bluetooth (77 bytes with
CRC32).

## CRC32 (Bluetooth)

CRC-32/IEEE (reflected polynomial `0xEDB88320`, initial value `0xFFFFFFFF`,
final XOR), computed over a two-byte prefix and then the report data without
its last 4 bytes. The result goes in those 4 bytes, little-endian. The
prefix is the Bluetooth HID transaction header followed by the report ID:

| Report | Prefix |
|--------|--------|
| Output | `0xA2`, id |
| Feature, set (DualSense) | `0x53`, id |
| Feature, set (DualShock 4, in this tester) | `0xA3`, id |
| Input / feature, received *(hid-playstation)* | `0xA1` / `0xA3`, id |

The two feature-set prefixes disagree between the tester's DualSense and
DualShock 4 code; check which one each pad accepts in C7.

## Feature reports

- `0x20` (DualSense): firmware information: build date and time (ASCII),
  firmware type, software series, hardware info, main, SBL and DSP firmware
  versions. Useful for detection (D3, P1) and not personal data.
- DualShock 4: firmware information in `0xA3`.
- `0x80` / `0x81` (DualSense): factory test commands (device id + action
  id, results polled from `0x81` in 56-byte pages). The tester uses them to
  read the **serial number, Bluetooth MAC address, PCBA ID, MCU unique ID,
  battery and motor barcodes**, and to drive audio output.
- `0x84` / `0x85`: "individual data" verification.
- Bluetooth pads start with a reduced report and switch to the full one
  (`0x31` / `0x11`) once the host reads a calibration or firmware feature
  report (C7 in ROADMAP/2026-10-10). The switch lasts until the pad is
  power-cycled (note 01, `SDL_JOYSTICK_ENHANCED_REPORTS`).

## Implications for anygamepad (C2, C7, W3, P1)

- C7 can take the offsets, output layout and CRC above as its starting
  tables (sans-IO, so the same code serves hidraw, Windows HID, IOKit and
  WebHID), checked against `hid-playstation`.
- AGENTS.md forbids collecting serial numbers and MAC addresses. Never send
  `0x80` test commands, in C7, W3 or reports. Read only `0x20` and the
  calibration report.
- W3 must not read feature reports from a Bluetooth pad without telling the
  user: doing so switches the pad to the full report for every application
  until it is power-cycled. Generic HID readers (the Gamepad API in Chrome on
  Windows and macOS, DirectInput games) may then see no input. This is
  unverified for Chrome; check on a device.
- On the web, the transport (C2 identity) can come from the input report
  sizes above, since WebHID does not report the bus.
- Reuse of code is allowed by the MIT license with attribution. Our drivers
  are Rust, so take the protocol facts, not the code.
