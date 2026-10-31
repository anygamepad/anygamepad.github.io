# Test plans

The steps a test mode session runs (roadmap task H3,
[ADR 0008](../../docs/adr/0008-test-mode-and-test-reports.md)). The web and
desktop apps embed these plans and run them with the library's test
runner (`anygamepad::testing`), which writes a test report
([`data/test-reports/`](../test-reports/README.md)).

- `quick.toml`: detection and every button, about a minute.
- `full.toml`: everything the first set of checks covers, about five
  minutes.
- One TOML file per plan, named after its `id`. Raise `version` whenever
  the steps change, so reports say which steps ran.
- `schema.json` is the JSON Schema of the format, generated from the Rust
  types in `crates/anygamepad-db` (`plan` module; never edit it by hand)
  and published at <https://anygamepad.github.io/schema/test-plan.json>.
  Regenerate it with
  `UPDATE_SCHEMA=1 cargo test -p anygamepad-db --all-features --test plans`.
- The build embeds every plan (`TestPlan::embedded`) and fails on an
  invalid one.

## Steps

A step has an `id` (lowercase words joined by dots), a `check`, and the
parameters that check takes. The runner expands steps for the model under
test, and leaves out what the model lacks.

| `check` | Parameters | What it asks and checks |
|---------|------------|-------------------------|
| `detection` | - | Nothing to do: what detection proposed equals what the person confirmed, and the device shows an identity the catalog expects for that mode and transport |
| `buttons` | - | One step per button of the model (`<id>.<button>`, as `controls.south`): press it and let go; passes when exactly that canonical button was pressed. South and east together say the pad does not have it |
| `stick_rest` | `side`, `seconds` (1.5), `tolerance` (0.15) | Leave the sticks alone; passes when the stick rests within `tolerance` of its centre |
| `stick_circle` | `side`, `tolerance` (0.9) | Turn the stick around its edge; passes when it reaches `tolerance` up, down, left and right; measures circularity |
| `trigger_pull` | `side`, `tolerance` (0.95) | Pull an analog trigger slowly, all the way; passes when it rests at 0, reaches `tolerance` and goes through values in between |
| `rate` | `seconds` (3) | Move both sticks; measures updates (and HID input reports) per second |
| `motion_rest` | `seconds` (1.5) | Lay the pad flat, face up; passes when the gyroscope rests near 0 and the accelerometer reads gravity on its up axis (`accel_y+`) |
| `motion_turn` | - | Turn the pad to the left on the table; passes when the gyroscope's up axis turns positive (`gyro_y+`) |
| `touchpad` | - | Touch the four corners, then use two fingers |
| `battery` | - | Nothing to do: the battery reports a state |
| `rumble`, `lightbar`, `player_leds` | - | The app drives the output (strong then weak motor; red, green, blue; players one to four); the person answers yes, no or not sure, on the screen or the pad (south, east, north) |
| `hotplug` | - | Unplug the pad or turn it off, then connect it again; passes when it comes back with the same identity |

Steps whose control the model lacks are left out (no `touchpad` step for
a pad without one). Steps the API cannot read (motion through the
Gamepad API) or outputs the provider cannot drive end as
`not_applicable`. Any step can be skipped, and any can be marked "not on
this pad".
