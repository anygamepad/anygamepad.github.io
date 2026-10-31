# Controller catalog

Controller models as sold, with their controls and the modes they can be
switched to (roadmap task H2,
[ADR 0008](../../docs/adr/0008-test-mode-and-test-reports.md)). The test
modes of the web and desktop apps list these models, show the ways to
connect each one, and detect which model and mode a connected pad is.

A catalog model is not a device profile
([`data/profiles/`](../profiles/README.md)): a profile says how to map one
identity to the canonical model; a catalog model says what a product is,
which modes it has and which identities each mode takes.

- One TOML file per model, at `<vendor>/<model>.toml`; the file's path
  without `.toml` is its `id` (`sony/dualsense`). Both parts are lowercase
  words of letters and digits joined by hyphens.
- `generic/` holds the generic models (`generic/xinput`,
  `generic/playstation`, `generic/switch-pro`, `generic/hid`) that stand
  for pads the catalog does not list. They have no identities: detection
  falls back on them by family.
- `schema.json` is the JSON Schema of the format, generated from the Rust
  types in `crates/anygamepad-db` (never edit it by hand) and published at
  <https://anygamepad.github.io/schema/controller.json>. Start a file with
  `#:schema ../schema.json` for completion and checks in editors that use
  Taplo (Even Better TOML).
- The build embeds every model in the library (`Catalog::embedded`), and
  fails on an invalid one. CI also checks every file against the schema,
  its path and a round trip:
  `cargo test -p anygamepad-db --all-features --test catalog`. After
  changing the types, regenerate the schemas with
  `UPDATE_SCHEMA=1 cargo test -p anygamepad-db --all-features --test catalog`.
- Identities left out are fine: a mode or a transport no one has seen yet
  has none, and the first test report that shows it fills it in. Write
  what a device shows, not what a manual claims.
- Never write anything about a person or a particular unit: those belong
  in the lab inventory ([`data/lab/`](../lab/README.md)), as codes.

## Format

A multi-mode pad, as an example of every field:

```toml
#:schema ../schema.json
schema_version = 1
id = "acme/pad-2"
vendor = "Acme"                # as printed on the box
name = "Pad 2 Wireless"        # as printed on the box
# generic = true               # only for generic/ models
# family = "xbox"              # the drawing it gets
labels = "nintendo"            # face labels, when the family does not tell
notes = "Two back buttons."    # never about a person or a unit

# What it has, in canonical names (ADR 0001), in every mode unless a mode
# has its own `controls`.
[controls]
buttons = [
    "south", "east", "west", "north",
    "dpad_up", "dpad_down", "dpad_left", "dpad_right",
    "left_shoulder", "right_shoulder", "left_trigger", "right_trigger",
    "left_stick", "right_stick", "select", "start", "home",
    "left_paddle_1", "right_paddle_1",
]
axes = ["left_stick_x", "left_stick_y", "right_stick_x", "right_stick_y", "left_trigger", "right_trigger"]
sensors = ["gyroscope", "accelerometer"]
touchpads = []                 # { max_fingers = 2, width = 1920, height = 1080 }
outputs = ["rumble"]
battery = true

# One table per mode, as printed on the switch or told in the manual.
[[modes]]
id = "x"                       # unique in the model
name = "X"                     # as printed
kind = "xinput"                # xinput, dinput, switch, ps3, ps4, ps5, xbox,
                               # android, apple, keyboard_mouse, other
how_to = "Hold X and Start to turn it on."

# One table per way to connect it in this mode.
[[modes.connections]]
transport = "dongle"           # usb, bluetooth, bluetooth_le, dongle
how_to = "Plug in the 2.4 GHz receiver, then turn the pad on."
# What a device connected this way shows; any of them. Conditions: vendor_id,
# product_id, name, os, backend, driver, descriptor_hash.
identities = [{ vendor_id = 0x045e, product_id = 0x028e }]

[[modes]]
id = "switch"
name = "S"
kind = "switch"
how_to = "Hold Y and Start to turn it on."
# The controls in this mode, when they differ.
# controls = { buttons = [...], axes = [...] }

[[modes.connections]]
transport = "bluetooth"
how_to = "Hold the pair button for three seconds, then pair it in the system's Bluetooth settings."
identities = [{ vendor_id = 0x057e, product_id = 0x2009 }]
```
