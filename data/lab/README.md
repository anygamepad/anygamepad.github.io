# Lab inventory

The physical controllers of the test lab and the machines they are tested
on (roadmap tasks H2 and F4,
[ADR 0008](../../docs/adr/0008-test-mode-and-test-reports.md)). The test
modes of the web and desktop apps list these units first, propose them
when a pad of their model connects, and show which of their modes and
transports are untested.

- `inventory.toml` is the lab's inventory (F4). The build embeds it in the
  library when it exists (`Inventory::embedded`) and fails if it names a
  model the catalog ([`data/controllers/`](../controllers/README.md)) does
  not have. Apps can also read an inventory from a file.
- `inventory.example.toml` shows the format; CI checks it.
- `schema.json` is the JSON Schema of the format, generated from the Rust
  types in `crates/anygamepad-db` (never edit it by hand) and published at
  <https://anygamepad.github.io/schema/inventory.json>.

## Codes

Each unit and each machine has a **code**: 1 to 16 ASCII letters, digits
and hyphens, starting with a letter or a digit (`DS5-02`, `XSX-01`,
`LINUX-1`). A suggested scheme for units is the model's short name and a
number. Codes cannot hold a person's name, by design.

Test reports name a unit by its code, so what a code means stays with the
person who gave it. They never name machines: a report says the OS, its
version numbers and the browser, nothing else about the computer.
Anything else here (notes) is about the pads and machines, never about a
person; serial numbers and Bluetooth addresses are never written, since
two pads of one model are told apart by their codes.

## Format

```toml
#:schema ./schema.json
schema_version = 1

[[unit]]
code = "DS5-02"
model = "sony/dualsense"       # a catalog model ID
notes = "Cosmic Red."          # edition, colour, firmware; optional

[[machine]]
code = "MAC-1"
os = "macos"                   # linux, windows, macos, android, ios
os_version = "15.1"            # optional
browsers = ["blink", "webkit"] # blink, gecko, webkit; optional
```
