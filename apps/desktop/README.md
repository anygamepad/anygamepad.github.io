# Desktop app

Detects controllers and exposes a virtual controller of a selectable type,
with an always-on-top widget and a collection mode.

Built with egui through eframe ([ADR 0004](../../docs/adr/0004-desktop-ui-toolkit.md)).
The shell (roadmap task A1: the main window with its pages, settings, the
system tray and the log), the Devices page (A2), collection mode (A3), the
test mode (H5) and the widget (A11). The virtual pad page arrives with the
virtual pad tasks.

```sh
cargo run -p anygamepad-desktop --release
```

The binary is `anygamepad`.

## Where it keeps things

| | Linux | Windows | macOS |
|---|---|---|---|
| Settings (`settings.toml`) | `$XDG_CONFIG_HOME/anygamepad` (`~/.config/anygamepad`) | `%APPDATA%\anygamepad` | `~/Library/Application Support/anygamepad` |
| Log (`anygamepad.log`, restarted at each launch) | `$XDG_STATE_HOME/anygamepad` (`~/.local/state/anygamepad`) | `%LOCALAPPDATA%\anygamepad\logs` | `~/Library/Logs/anygamepad` |
| Device reports (`reports/`) and test reports (`test-reports/`) | `$XDG_DATA_HOME/anygamepad` (`~/.local/share/anygamepad`) | `%APPDATA%\anygamepad` | `~/Library/Application Support/anygamepad` |

The log never holds serial numbers, Bluetooth addresses or device paths.

## System tray

Linux: StatusNotifierItem over D-Bus (KDE, most desktops; GNOME needs the
AppIndicator extension). Windows and macOS: the native tray. Without a
tray the window stays open, and the tray settings are disabled.

## Runtime libraries on Linux

The window needs `libxkbcommon-x11` (X11) or `libwayland-client` and
`libxkbcommon` (Wayland), and OpenGL. Desktop distributions ship them.

## Icon

`assets/icon-64.rgba` is the logo's mark (`apps/web/public/favicon.svg`)
rendered at 64×64, raw RGBA.

## Test mode

The Test page (roadmap task H5,
[ADR 0008](../../docs/adr/0008-test-mode-and-test-reports.md)) runs the
library's test runner on a connected controller, with the web tester's
flow:

1. The list of controllers: the units of the lab inventory, then the
   catalog's models. A controller connected (or the one connected when the
   page opens) is detected and shown at once: **Confirm**, or **Choose
   another** when it does not match (south and east on the pad do the
   same).
2. Picking a model shows the ways to connect it (mode and transport, with
   how to get there; "Tested here" once a report of it is saved). Then
   connect it, press a button on it, or pick it among the connected pads;
   the app says when it looks like something else.
3. A plan: **Quick** (detection and every button), **Full** (sticks,
   triggers, update rate, motion, touchpad, battery, outputs, unplugging)
   or **Virtual pads** (desktop only, below). The page shows what to do;
   Skip, Not on this pad, Nothing happens, the answers (also from the pad:
   south yes, east no, north not sure) and Stop.
4. The summary: results, notes, the texts of the report to check before
   sharing it (names the catalog does not know are marked), **Save** to
   `test-reports/` (see above) and **Open the folder**.

The device worker runs the session, so its timing does not depend on the
window. Reports are anonymous (no serial numbers, addresses or paths) and
stay on this computer.

**Virtual pads plan:** for each type of virtual pad (Xbox 360 Controller,
DualShock 4, DualSense, Switch Pro Controller) the app creates one, shows a
series of states on it (each button, each stick axis at both ends, each
trigger) and reads them back through its own provider, as a game would;
then a game's vibration sent to the virtual pad must reach the pad under
test (the person answers). On Linux it needs write access to
`/dev/uinput` and `/dev/uhid` ([docs/linux-permissions.md](../../docs/linux-permissions.md));
where they are missing or not writable, or on Windows and macOS for now,
each type ends as not applicable with the reason.

`ANYGAMEPAD_DEMO=1` runs sessions on the demo pad: in a session it follows
the prompts by itself (presses what is asked, turns the sticks, answers
yes, unplugs and comes back).

## Widget

Settings, Widget: a small window that shows one controller above other
windows (roadmap task A11), drawn as a controller or compactly, with an
adjustable background and optional click-through (turn it off to drag the
widget). Limits:

- **Wayland**: regular windows cannot keep themselves on top; use the
  window menu's "Always on top" where the desktop has it (KDE, most
  wlroots desktops), or run the app under XWayland
  (`WAYLAND_DISPLAY= anygamepad`). A layer-shell widget is a later step.
- **X11**: the window manager decides; most honour the request.
- **Transparency** needs an OpenGL configuration with alpha and, on X11, a
  compositor; without them the widget is opaque.
- **Exclusive full-screen games** cover every window on every OS: play in
  borderless windowed mode to see the widget.
