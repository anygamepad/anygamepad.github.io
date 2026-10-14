# anygamepad-db

Device profile format and embedded device database for anygamepad.

A profile says which devices it is for (match rules on the identity a
provider reports) and how their raw elements map to the canonical gamepad
model. Profiles are TOML (`data/profiles/`, described by a JSON Schema),
and SDL GameControllerDB entries convert into profiles.

Features: `toml` (read and write TOML profiles), `schemars` (the JSON
Schema).

Part of [anygamepad](https://github.com/anygamepad/anygamepad.github.io).
Not usable yet: the project is in its first development phase.
