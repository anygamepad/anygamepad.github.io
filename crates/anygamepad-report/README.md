# anygamepad-report

Device report schema shared by the anygamepad apps and server.

A report is what the web tester or the desktop app's collection mode saw of
one controller: the app and host, what the person says about the device,
and one view per provider or browser API (identity, fingerprint, raw
layout, HID structure and sample input reports, confirmed mapping). The
same JSON document goes to a file, the browser's IndexedDB, a GitHub issue,
and later the sync server (`data/reports/`, described by a JSON Schema).

Reports never contain serial numbers, Bluetooth MAC addresses or other
personal data: `Report::scrub` removes what looks like them and
`Report::validate` rejects it.

Features: `json` (read and write JSON reports), `schemars` (the JSON
Schema).

Part of [anygamepad](https://github.com/anygamepad/anygamepad.github.io).
Not usable yet: the project is in its first development phase.
