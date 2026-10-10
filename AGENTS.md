# AGENTS.md

Instructions for any agent (human or AI) working in this repository.

## Project

rustpad normalizes gamepad input. Many controllers expose several modes
(XInput, DirectInput, Switch Pro Controller, DualShock 4/DualSense emulation,
vendor modes such as GuliKit or 8BitDo), and the same controller can identify
itself differently on Windows, Linux and macOS or over USB vs Bluetooth.
rustpad hides all of that behind one canonical gamepad model.

The repository and project are named rustpad; the library crates are published
as `omnipad` / `omnipad-*` (`rustpad` is taken on crates.io).

Deliverables:

1. **Rust library** that Rust games import to get normalized gamepad input,
   isolating them from device, mode, transport and OS quirks.
2. **Desktop app** (Windows, Linux, macOS) that detects controllers and exposes
   a virtual controller of a selectable type (Xbox 360, DualShock 4, DualSense,
   Switch Pro Controller, ...), similar to DS4Windows. Its UI is
   performance-first and includes an always-on-top widget.
3. **Web app** similar to hardwaretester.com/gamepad, built on the Rust
   library compiled to WebAssembly and connected to a database that collects
   reports of detected controllers. Those reports are used to improve
   controller detection in later iterations.
4. **Collection mode** in the desktop app that gathers device reports too.

Priority: the web app comes before the desktop app.

The library aggregates several input providers (native backends, web backend,
SDL3, gilrs) and arbitrates between them per device, to support as many
gamepads as possible.

The current plan is in the most recent directory under `ROADMAP/`.

## Language

The following must **always be written in English**, regardless of the
language used to talk to the user:

- `AGENTS.md`
- Everything under `ROADMAP/`
- Everything under `RECORD/`
- Code comments (including doc comments)

Conversation with the user may happen in any language.

## Session workflow

1. Read this file.
2. Read the latest roadmap: `ROADMAP/<most recent date>/README.md` and the
   detailed file(s) relevant to the task.
3. Read every `RECORD/*.WIP.md` file and the most recent `*.completed.md`
   records related to the task.
4. Create or update a RECORD file for the work you do (see below).
5. Before finishing, update the RECORD file and rename it to `completed` if the
   work is done.

## ROADMAP

- Location: `ROADMAP/AAAA-MM-DD/` (ISO date `YYYY-MM-DD`, the date the roadmap
  version was written), containing Markdown files.
- `README.md` in each roadmap directory is the entry point: vision, master
  schedule, dependency graph, milestones, risks.
- Detailed files are numbered by workstream (`00-foundations.md`,
  `01-core-library.md`, ...).
- Every roadmap file must include a **Mermaid Gantt diagram** that shows which
  tasks block which:
  - Each task has an ID (e.g. `c4`) and declares its blockers with
    `after <id> <id> ...`.
  - Tasks on the critical path are tagged `crit`.
  - Blockers that live in another file are drawn as milestones in an
    `Upstream blockers` section.
  - A table lists for each task: ID, duration, **Blocked by**, **Blocks**,
    start, end and slack.
- Task IDs are stable within a roadmap version and are referenced from RECORD
  files and commit messages.
- A roadmap version is a snapshot. To re-plan, create a new dated directory
  (copying and editing the previous one) instead of rewriting history. Small
  corrections (typos, status updates), and re-plans made on the same day the
  version was created, may be applied in place; record them in a RECORD file. When a new
  version is created, mark the previous `README.md` status as
  `superseded by ROADMAP/<new date>/`.

## RECORD (project memory)

RECORD files are the shared memory between sessions and agents.

- Location and name: `RECORD/AAAA-MM-DD.<summary>.(WIP|completed).md`
  - `AAAA-MM-DD`: ISO date (`YYYY-MM-DD`) when the work started.
  - `<summary>`: short, lowercase, kebab-case description
    (e.g. `linux-evdev-backend`).
  - `WIP` while the work is in progress, `completed` once it is done.
- Example: `RECORD/2026-10-10.initial-roadmap-and-agent-rules.completed.md`.
- When the work is done, rename the file with `git mv` from `.WIP.md` to
  `.completed.md`; do not create a second file.
- One RECORD per unit of work. Keep them concise and factual.
- Required sections:

  ```markdown
  # <Title>

  - Status: WIP | completed
  - Roadmap: ROADMAP/<date>/ (task IDs: C4, D3, ...)

  ## Goal
  ## Done
  ## Decisions
  ## Open questions
  ## Next steps
  ```

- Record decisions together with their reason, so later sessions do not
  re-litigate them.

## Code conventions

- Rust workspace; format with `cargo fmt`, lint with
  `cargo clippy -- -D warnings`, test with `cargo test`.
- Every `unsafe` block needs a `// SAFETY:` comment explaining why it is sound.
- Platform-specific code is isolated behind `cfg(target_os = ...)` modules.
- Never collect or log serial numbers, Bluetooth MAC addresses or other
  personal data from devices or users.
- Architecture decisions go to `docs/adr/NNNN-title.md`.
