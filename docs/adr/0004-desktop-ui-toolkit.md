# 0004 — Desktop UI toolkit

- Status: proposed (needs the prototype measurements below before acceptance)
- Date: 2026-10-10
- Roadmap: F3 (ADR 4); used by A1, A11

## Context

The desktop app must be performance-first (low input-to-pixel latency, low
idle CPU and memory) and have an always-on-top widget (A11) that can be
transparent, undecorated and click-through. The roadmap recommends egui.
Desktop work starts after M2, so this decision is not urgent.

## Decision

**egui** through `eframe` with the `wgpu` renderer, in the same process as
the library:

- Pure Rust, no IPC or webview between input and UI.
- Immediate mode suits a live view redrawn at the display refresh rate, and
  it can stop repainting when idle.
- Multiple native windows (viewports) with per-window always-on-top,
  transparency, no decorations and mouse pass-through through winit.
- The controller visualization can be shared with the web app if ADR 0005
  ever embeds an egui canvas.

**Acceptance criteria** (prototype on Linux X11 and Wayland, Windows, macOS):
a main window and an always-on-top transparent widget showing one live pad.

| Metric | Target |
|--------|--------|
| Idle CPU (no input, widget visible) | < 1 % of one core |
| Resident memory | < 100 MB |
| Input-to-pixel latency | ≤ 1 frame beyond the input poll |
| Widget | always on top, transparent, click-through where the platform allows; Wayland behaviour documented |

## Alternatives

- iced: similar performance, less mature multi-window support.
- Slint: good performance; its licence options need review for this
  project.
- Tauri: webview memory/CPU cost and fragile transparent overlay windows on
  Linux; rejected.

## Consequences

- If the prototype misses a target, revisit iced or Slint before A1 starts.
- Wayland lacks a standard always-on-top for normal windows; the layer-shell
  fallback is part of A11.
