# Device profiles

Curated device profiles, embedded in the anygamepad library (D4). Each
profile says which devices it is for and how their raw buttons, axes and
hats map to the canonical model (`docs/adr/0001-canonical-model-naming.md`).

- One TOML file per device family (ADR 0003), named after it
  (`dualsense.toml`). Comment why a quirk exists and which report or device
  confirmed it.
- `schema.json` is the JSON Schema of the format, generated from the Rust
  types in `crates/anygamepad-db` (never edit it by hand) and published at
  <https://anygamepad.github.io/schema/profile.json>. Start a file with
  `#:schema ./schema.json` for completion and checks in editors that use
  Taplo (Even Better TOML).
- CI parses every profile, validates it and checks it against the schema:
  `cargo test -p anygamepad-db --all-features`. After changing the types,
  regenerate the schema with
  `UPDATE_SCHEMA=1 cargo test -p anygamepad-db --all-features --test profiles`.
- SDL GameControllerDB entries are not copied here: they are converted from
  `data/sdl-gamecontrollerdb/` (D2). To start a profile from one, print it
  with `cargo run -p anygamepad-db --features toml --example sdl_import --
  data/sdl-gamecontrollerdb/gamecontrollerdb.txt 054c:09cc`.

## Format

```toml
#:schema ./schema.json
schema_version = 1
name = "DualShock 4"
source = "curated"        # or "report" (P4), "sdl-gamecontrollerdb" (D2)
verified = true           # checked on real hardware
family = "dualshock4"     # optional; gives the face labels
# face = { south = "cross", east = "circle", west = "square", north = "triangle" }
# indices = "sdl"         # SDL joystick indices instead of the backend's
# attribution = "..."     # origin and license of imported data

# The devices the profile is for: any rule may hold; within a rule, every
# field that is set must hold.
[[match]]
vendor_id = 0x054c
product_id = 0x05c4

[[match]]
vendor_id = 0x054c
product_id = 0x09cc

# Mapping entries: those whose `when` holds apply in order, and a later
# entry replaces the targets it sets.
[[mapping]]
buttons = { south = "b0", east = "b1" }
axes = { left_stick_x = "a0", left_stick_y = "a1~", right_trigger = "b7" }

[[mapping]]
when = { os = "macos", backend = "iokit_hid" }
buttons = { south = "b1", east = "b2" }
```

### Match rules

Fields of `[[match]]` and `when`, all optional, compared with the device
identity a provider reports: `vendor_id`, `product_id` (needs
`vendor_id`), `version`, `name` (exact product name), `driver` (`xpad`,
`hid-playstation`, ...), `os` (`linux`, `windows`, `macos`, `android`,
`ios`), `backend` (`evdev`, `hidraw`, `windows_hid`, `xinput`,
`direct_input`, `iokit_hid`, `game_controller`, `gamepad_api`, `webhid`,
...), `transport` (`usb`, `bluetooth`, `bluetooth_le`, `dongle`), `browser`
(`blink`, `gecko`, `webkit`), `gamepad_mapping` (`"standard"`, `""`),
`descriptor_hash` (16 hexadecimal digits) and `sdl_guid` (32).

When several profiles match a device, the best is, in turn: one whose rule
names the device (`product_id`, `name`, `descriptor_hash` or `sdl_guid`)
over one that names a class of devices; verified over unverified; curated
over reports over SDL GameControllerDB; then the rule with more fields.

### Inputs

Targets are canonical names: buttons (`south`, `east`, `west`, `north`,
`dpad_up`, ..., `left_shoulder`, `left_trigger`, `left_stick`, `select`,
`start`, `home`, `share`, `capture`, `mute`, `left_paddle_1`, ...,
`touchpad_click`, `extra_0` to `extra_31`) and axes (`left_stick_x`,
`left_stick_y`, `right_stick_x`, `right_stick_y`, `left_trigger`,
`right_trigger`). Inputs use SDL GameControllerDB's syntax:

| Input | Reads |
|-------|-------|
| `b3` | button 3 |
| `a1` | axis 1, from its minimum to its maximum |
| `+a1`, `-a1` | half of axis 1, from the centre to its maximum or minimum |
| `a1~`, `-a1~` | the same, reversed |
| `h0.4` | hat 0 pointing towards a direction of the mask: 1 up, 2 right, 4 down, 8 left |

A number counts the backend's elements in its order (or SDL's, with
`indices = "sdl"`). Other raw sources go in braces: `b{evdev:0x0130}`,
`a{hid:0x0001:0x0030}`, `h{hid:0x0001:0x0039}.1`.

A button is pressed while its input is at least halfway through its range
(a button input: while the raw button is pressed). A stick axis goes from -1
to 1 over its input's range, a trigger from 0 to 1. Stick Y is positive
**up**; most backends report it positive down, hence `a1~`. An axis can
also take a table: `{ negative = "b14", positive = "b15" }` for halves
driven separately, or `{ input = "a5", deadzone = 0.05 }`.
