# 0008 — Test mode and test reports

- Status: proposed (2026-10-31)
- Date: 2026-10-31
- Roadmap: H1-H10, F4, D3, C8, P4 (ROADMAP/2026-10-31/)

## Context

Nine tasks (C15, W3, C4, C7, A1, A2, A3, A5, A11) have their code merged
but were never tried on a real controller. Each was to be checked by hand
with its own instructions (an example program, a recording button, a
checklist written in a chat), and the results would live in chat
messages: nothing repeatable, no evidence, nothing that catches a later
regression.

The owner collects controllers (several DualSense, Xbox Series, DualShock 4,
DualShock 3, 8BitDo, GuliKit, GameSir and other brands) and can test them on
Windows, Linux and macOS. The apps that read controllers keep growing: the
web tester (Gamepad API, WebHID), the desktop app (evdev, hidraw), and later
the Windows and macOS backends, SDL3, gilrs and `germadev/motorust`.

Device reports (P1, [ADR 0007](0007-local-first-device-reports.md)) record
how a device is seen, not whether it works: what the person says about it
is optional, there are no results per control, and a report must fit a
GitHub issue body (60,000 bytes).

The owner asked on 2026-10-31 for a test mode in every app: the person
picks the controller or the kind of controller, and the app writes a
report file (a download on the web), in a way that is robust, comfortable
and extensible. The same day the owner added that the app should also
detect the controller by itself, besides the list, and that the reports
should be anonymous.

## Decision

1. **A test mode in every app, one runner in the library.** The runner
   (`anygamepad::testing`, H3) is a sans-IO state machine like the guided
   wizard (W4), whose detection rules it reuses: fed with raw states,
   canonical states and provider events, it decides the checks, records
   the evidence and builds the report. Apps show its prompts, send the
   outputs it asks for and save the report. The web app runs it in
   WebAssembly.
2. **Test plans are data, kinds of check are code.** Plans
   (`data/test-plans/`, Quick and Full first) list steps, each a kind of
   check with parameters and the conditions under which it applies; they
   are validated by a schema and embedded in the apps. A new kind of check
   is written once in the runner and every app gets it.
3. **A controller catalog and a lab inventory** (H2). The catalog
   (`data/controllers/`) describes models as sold: controls, sensors,
   outputs, and modes with how to switch to them, their transports and the
   identity expected in each, plus generic types for unlisted pads. It is
   separate from device profiles (D1), which say how to map an identity;
   models point to the profiles of their modes. The inventory
   (`data/lab/inventory.toml`, F4) lists the units, the physical pads,
   under labels their owner gives them, and the test machines. Both are
   embedded in the apps; an inventory file can also be imported.
4. **A test report is its own document** (`anygamepad-test-report`, H1)
   that embeds the device report (P1) of the device under test, and adds the
   configuration the person set (unit, catalog model, mode, transport: the
   ground truth) and what automatic detection proposed, the environment
   (OS, app, API or provider), the plan, a
   result per step (status, expected, observed, measurements) and the
   evidence (the input of each step's window, as raw states and, where the
   API reads HID, input reports). It follows the device report's privacy
   rules (no serial numbers, MAC addresses or personal data; scrubbed by
   clients, validated by every reader) and adds no time of day. It is a
   file, not an issue body; H1 sets its size budget.
5. **Stored locally, committed by choice.** The desktop app writes
   `test-reports/` in its data directory; the web app keeps sessions and
   reports in IndexedDB (store `test-reports`) and downloads them. Nothing
   leaves by itself (ADR 0007). The reports the owner chooses are
   committed to `data/test-reports/`, under CC0 like device reports.
6. **The list first, automatic detection when connecting without picking**
   (H10). The test mode opens on the list of controllers (inventory units,
   catalog models, generic types); picking one shows its ways to connect
   (one per mode and transport, with the catalog's instructions), and the
   app checks the pad that connects against that choice. A pad connected
   without picking is shown as detected, with Confirm and Choose another
   (owner, 2026-10-31). For that, the library proposes the device (the only one connected, or the one whose button is
   pressed), its model and mode (a catalog lookup on the provider's
   identity, candidates most likely first, a generic type when nothing
   matches), its transport (from the provider, asked when the API does not
   tell) and the unit (the inventory's units of that model). The person
   confirms with one press or corrects from the list. Units of one model
   are told apart by the person, not by serial numbers, which are never
   read. Both the proposal and the confirmation go into the report, so
   detection is measured like any other check, and C8 later improves the
   lookup behind the same call.
7. **Anonymous by construction.** A test report has one form, and it never
   identifies the person, their machine or a pad beyond the unit code they
   chose: the device report's rules (no serial numbers, MAC addresses,
   device paths, personal data; identifying input reports never sampled,
   in the evidence too), no time of day, unit codes restricted to letters,
   digits and hyphens, OS versions cut to their numbers, input recorded
   only during the steps, notes only when kept. Device names the catalog
   does not know for the model (a person can rename a Bluetooth pad) are
   shown in a review of every text before saving, computed by the library.
   What is private but useful (what the codes mean, which machine, the
   last unit picked) stays in the app's local store.
8. **What test reports are for:** closing tasks (the first test campaign,
   H6), regression tests that replay committed reports in CI (H7), a
   coverage matrix and a public compatibility page (H8), profiles marked
   verified by the reports that pass them (D3), mode and clone detection
   (C8), and curation (P4).

## Alternatives

- **Checklists in the docs, results by hand.** Nothing to build, but no
  evidence, no regression tests, and it does not scale to a collection
  tested on three OSes and several apps.
- **Test results inside the device report.** One format, but device
  reports must fit an issue body and are meant for anyone to share, while
  test evidence is large and comes from a lab. A test report embeds a
  device report instead, so the two never drift.
- **Test logic in each app** (TypeScript on the web, Rust on the desktop).
  Duplicated logic that drifts between apps; the wizard already showed
  that a shared state machine works in both.
- **Test plans as code only.** Simpler at first, but plans as data are
  readable without Rust, validated by a schema, shown on the compatibility
  page, and edited without touching the runner.
- **Device reports only, matched to a separate spreadsheet of results.**
  No ground truth tied to the input, nothing to replay.
- **Choosing the controller only from a list.** Simple, but slow over a
  collection, and it never tests detection itself.
- **Telling units apart by serial number or Bluetooth address**, which
  many pads expose. Automatic, but both are personal data (AGENTS.md); a
  code the person gives is enough.
- **Two forms of report, full and anonymized.** A full form would exist on
  disk and could be shared by mistake; with one anonymous form there is
  nothing to anonymize afterwards.
- **A local-only inventory and reports.** More private; but the coverage
  matrix, the compatibility page and the replay tests need them in the
  repository. The import keeps the local way open (see Consequences).

## Consequences

- A new workstream in ROADMAP/2026-10-31/ (H1-H9, milestone M6). The nine
  tasks above stay in progress until the first test campaign (H6), and the
  critical path runs through H2 -> H3 -> H4/H5 -> H6: M4 moves to
  2027-02-03.
- D3 waits for H6, C8 for H9 and P4 for H1: profiles and mode detection
  are built on verified data.
- The repository grows with the committed test reports; H1's size budget
  and choosing which reports to commit keep it small. If it still grows
  too much, the evidence can move to release assets.
- Committing the inventory and the owner's test reports makes the list of
  the owner's controllers public, as models and unit codes; the reports
  themselves say nothing about the person. The owner can keep the
  inventory local (imported in the apps) instead, and the compatibility
  page then lists models only.
- Detection accuracy per model and mode becomes visible in the coverage
  matrix (H8): the measure C8 has to improve.
- The catalog becomes where knowledge about modes lives (how to switch,
  what each mode looks like), which the compatibility page shows to users.
- Any later app or provider joins by feeding the runner; its reports need
  no new tooling.
