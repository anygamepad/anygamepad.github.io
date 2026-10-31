# Test reports

What a test mode session found about one controller, in one
configuration (unit, model, mode, transport) and one environment (OS, app,
API): roadmap task H1,
[ADR 0008](../../docs/adr/0008-test-mode-and-test-reports.md). The web
tester downloads them; the desktop app writes them to `test-reports/` in
its data folder. The ones the lab chooses to keep are committed here,
where the replay tests (H7) and the compatibility matrix (H8) read them.

- `schema.json` is the JSON Schema of the format, generated from the Rust
  types in `crates/anygamepad-report` (`test_report` module; never edit it
  by hand) and published at
  <https://anygamepad.github.io/schema/test-report.json>.
- `example.json` is a Quick session on a DualSense over USB through the
  Gamepad API in Chrome on Windows; its values are illustrative. The tests
  regenerate it with the schema:
  `UPDATE_SCHEMA=1 cargo test -p anygamepad-report --all-features --test test_report`.
- File names: `<unit or model>-<mode>-<transport>-<os>-<app>-<api>-<day>-<n>.json`
  (`ds5-01-ps5-usb-windows-web-gamepad_api-2026-10-14-1.json`).
- A report is at most 4 MiB as compact JSON. A Full session of 45 steps
  takes about 2.2 MB from a pad read over HID at 250 reports per second,
  and 0.8 MB from the Gamepad API.
- Committed reports are dedicated to the public domain under
  [CC0 1.0](../reports/LICENSE-CC0), like device reports.

## Contents

- `device`: the device report (P1, [`data/reports/`](../reports/README.md))
  of the device under test: the day, the app, the host, and how each API
  saw the device. The first view is the API the session used.
- `configuration`: how the session started (`list` or `connecting`), what
  the person `confirmed` (the ground truth: unit code, catalog model and
  mode from [`data/controllers/`](../controllers/README.md), mode kind,
  transport), and what automatic detection proposed (`detected`).
- `plan`: the test plan's ID and version.
- `results`: one per step, in order: the step and its kind of check, its
  `status` (`pass`, `fail`, `skipped`, `not_applicable`, `error`), what was
  `expected` and `observed`, the person's `answer` for checks only a person
  can make, `measurements`, how long it took, a note, and the `evidence`:
  the input recorded during the step, as input reports when the API reads
  HID, as raw states otherwise, with times in milliseconds from the
  step's start.
- `notes`: the person's note on the session.

## Anonymous by construction

There is one form of test report, and it never identifies the person,
their machine or a pad beyond the unit code they chose:

- nothing a device report rules out: serial numbers, Bluetooth addresses,
  device paths, personal data, nor the input reports that can carry an
  address (Nintendo's subcommand and USB command replies), in the evidence
  too;
- the day, never the time;
- units by their code (letters, digits and hyphens), never free text, and
  no `user.product`: the configuration says what the pad is;
- the OS version cut to its numbers (`6.8.0-45-generic` becomes `6.8.0`);
- notes only when the person keeps them, checked like every text.

The apps scrub every report they build (`TestReport::scrub`), and show
every text it holds before saving (`TestReport::review`), flagging device
names the catalog does not know for the model, since a person can rename
a Bluetooth pad. Every reader validates (`TestReport::validate`); CI
rejects a committed report that is not anonymous.
